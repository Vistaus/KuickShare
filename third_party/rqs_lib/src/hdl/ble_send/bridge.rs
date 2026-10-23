//! Hardware-free bridge between an L2CAP channel and the byte stream an `OutboundRequest`
//! uses; the central-side counterpart of `ble_receiver::bridge::BleBridge`.
//!
//! Us to peripheral: the stream carries `[4-byte BE len][OfflineFrame]`; every complete
//! frame becomes one data message (split into SDUs of at most 16 KB).
//! Peripheral to us: data messages are acknowledged and written to the stream.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::time::{Instant, sleep_until, timeout};

use super::l2cap::{Event, L2capSession};

/// The peripheral has to accept the data connection this soon after we ask.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// After the handshake the peripheral has to send its first data this soon (its UKEY2
/// server init); later silence is normal, e.g. while the user decides on the phone.
pub const FIRST_DATA_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a closing bridge keeps handing over what it already holds.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the link gets to shut down.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
/// Bytes from the peripheral waiting for the session to read them.
const MAX_PENDING_TO_STREAM: usize = 1 << 20;
/// SDUs queued before the stream is no longer read (the link is the slow side).
const MAX_OUTBOX: usize = 64;
const READ_CHUNK: usize = 16 * 1024;
/// Largest frame accepted from the session (file chunks are about 512 KB).
const MAX_FRAME: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeExit {
    /// The peripheral closed the socket.
    PhoneDisconnected,
    /// The session closed or dropped its end (also what a finished upgrade looks like).
    StreamClosed,
    /// The peripheral answered the data connection request with the failure byte.
    Refused,
    /// No answer to the data connection request in time.
    HandshakeTimeout,
    /// Connected, but no data from the peripheral in time.
    SilentPeer,
    /// The peripheral or the session broke the protocol.
    Protocol(String),
    /// The link went away (read or write error, end of file).
    LinkClosed,
}

#[derive(Debug)]
pub struct CentralBridge<L> {
    link: L,
    stream: DuplexStream,
    handshake_timeout: Duration,
    first_data_timeout: Duration,
}

impl<L: AsyncRead + AsyncWrite + Unpin + Send> CentralBridge<L> {
    /// `link` is the connected L2CAP channel. Dropping the bridge drops `stream`, which the
    /// session sees as end of stream.
    pub fn new(link: L, stream: DuplexStream) -> Self {
        Self {
            link,
            stream,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            first_data_timeout: FIRST_DATA_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub fn with_handshake_timeout(mut self, t: Duration) -> Self {
        self.handshake_timeout = t;
        self
    }

    #[cfg(test)]
    pub fn with_first_data_timeout(mut self, t: Duration) -> Self {
        self.first_data_timeout = t;
        self
    }

    /// Runs the handshake and then the session. Messages already produced are written to
    /// the link first, so the peripheral gets the last frame (e.g. the plaintext
    /// DISCONNECTION of an upgrade) and our socket DISCONNECTION.
    pub async fn run(self) -> BridgeExit {
        let Self {
            link,
            stream,
            handshake_timeout,
            first_data_timeout,
        } = self;
        let (mut link_rd, mut link_wr) = tokio::io::split(link);
        let (mut reader, mut writer) = tokio::io::split(stream);
        let mut core = Core::default();
        core.urgent.push_back(core.session.request());
        let mut stream_chunk = vec![0u8; READ_CHUNK];
        let mut link_chunk = vec![0u8; READ_CHUNK];
        let handshake_at = Instant::now() + handshake_timeout;
        let mut silent_at = handshake_at;
        let mut flush_at: Option<Instant> = None;

        // Every write is one branch of the loop, not an await inside another one: the session
        // may be blocked writing to us while we would be blocked writing to it, and the same
        // goes for the link.
        while !core.is_done() {
            core.refill();
            if core.exit.is_some() && flush_at.is_none() {
                flush_at = Some(Instant::now() + FLUSH_TIMEOUT);
            }
            tokio::select! {
                read = link_rd.read(&mut link_chunk),
                    if core.exit.is_none() && core.to_stream.len() < MAX_PENDING_TO_STREAM =>
                {
                    match read {
                        Ok(n) if n > 0 => {
                            silent_at = Instant::now() + first_data_timeout;
                            core.on_link_bytes(&link_chunk[..n]);
                        }
                        _ => core.end(BridgeExit::LinkClosed),
                    }
                }
                read = reader.read(&mut stream_chunk),
                    if core.exit.is_none() && core.ready() && core.outbox.len() < MAX_OUTBOX =>
                {
                    match read {
                        Ok(n) if n > 0 => core.on_stream_bytes(&stream_chunk[..n]),
                        _ => core.end(BridgeExit::StreamClosed),
                    }
                }
                written = writer.write(&core.to_stream), if !core.to_stream.is_empty() => {
                    match written {
                        Ok(n) => {
                            core.to_stream.drain(..n);
                        }
                        Err(_) => core.end(BridgeExit::StreamClosed),
                    }
                }
                written = link_wr.write(&core.to_link), if !core.to_link.is_empty() => {
                    match written {
                        Ok(n) if n > 0 => {
                            core.to_link.drain(..n);
                        }
                        _ => core.abort_link(),
                    }
                }
                () = sleep_until(handshake_at), if !core.ready() && core.exit.is_none() => {
                    core.end(BridgeExit::HandshakeTimeout);
                }
                () = sleep_until(silent_at),
                    if core.ready() && !core.got_data && core.exit.is_none() =>
                {
                    core.end(BridgeExit::SilentPeer);
                }
                () = sleep_until(flush_at.unwrap_or(handshake_at)), if flush_at.is_some() => {
                    break;
                }
            }
        }
        let _ = timeout(SHUTDOWN_TIMEOUT, link_wr.shutdown()).await;
        core.exit.unwrap_or(BridgeExit::StreamClosed)
    }
}

/// The synchronous part of the bridge: no I/O, so every transition can be tested directly.
#[derive(Debug, Default)]
struct Core {
    session: L2capSession,
    /// The peripheral has sent at least one data message.
    got_data: bool,
    /// Peripheral data not yet written to the stream.
    to_stream: Vec<u8>,
    /// Partial frame read from the stream.
    from_stream: Vec<u8>,
    /// Handshake, acknowledgements: go out before data.
    urgent: VecDeque<Vec<u8>>,
    /// SDUs of session frames.
    outbox: VecDeque<Vec<u8>>,
    /// The SDU being written to the link.
    to_link: Vec<u8>,
    exit: Option<BridgeExit>,
}

impl Core {
    fn ready(&self) -> bool {
        self.session.is_ready()
    }

    fn is_done(&self) -> bool {
        self.exit.is_some()
            && self.urgent.is_empty()
            && self.outbox.is_empty()
            && self.to_link.is_empty()
            && self.to_stream.is_empty()
    }

    fn refill(&mut self) {
        if self.to_link.is_empty()
            && let Some(sdu) = self.urgent.pop_front().or_else(|| self.outbox.pop_front())
        {
            self.to_link = sdu;
        }
    }

    fn clear_link_side(&mut self) {
        self.urgent.clear();
        self.outbox.clear();
        self.to_link.clear();
    }

    fn abort_link(&mut self) {
        self.clear_link_side();
        self.end(BridgeExit::LinkClosed);
    }

    /// The first reason wins. What is still worth delivering depends on who ended it.
    fn end(&mut self, exit: BridgeExit) {
        if self.exit.is_some() {
            return;
        }
        match exit {
            // Let the session read the rest of what the peripheral sent.
            BridgeExit::PhoneDisconnected | BridgeExit::LinkClosed => self.clear_link_side(),
            // The session is gone, but the peripheral still gets its last frames and a
            // socket DISCONNECTION.
            BridgeExit::StreamClosed => {
                self.to_stream.clear();
                if let Some(goodbye) = self.session.goodbye() {
                    self.outbox.push_back(goodbye);
                }
            }
            _ => {
                self.to_stream.clear();
                self.clear_link_side();
            }
        }
        self.exit = Some(exit);
    }

    fn on_link_bytes(&mut self, bytes: &[u8]) {
        for event in self.session.feed(bytes) {
            match event {
                Event::Send(sdu) => self.urgent.push_back(sdu),
                Event::Ready => debug!("l2cap bridge: data connection ready"),
                Event::Data(d) => {
                    self.got_data = true;
                    self.to_stream.extend_from_slice(&d);
                }
                Event::Disconnected => self.end(BridgeExit::PhoneDisconnected),
                Event::Failed(reason) => {
                    warn!("l2cap bridge: {reason}");
                    self.end(BridgeExit::Protocol(reason));
                }
                Event::Refused => self.end(BridgeExit::Refused),
            }
        }
    }

    fn on_stream_bytes(&mut self, bytes: &[u8]) {
        self.from_stream.extend_from_slice(bytes);
        while let Some(prefix) = self.from_stream.first_chunk::<4>() {
            let len = u32::from_be_bytes(*prefix) as usize;
            if len > MAX_FRAME {
                self.end(BridgeExit::Protocol(
                    "oversized frame from the session".into(),
                ));
                return;
            }
            if self.from_stream.len() < 4 + len {
                return;
            }
            let frame: Vec<u8> = self.from_stream.drain(..4 + len).collect();
            self.outbox.extend(self.session.queue_data(&frame));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hdl::ble_receiver::socket::data_message;
    use crate::hdl::ble_send::l2cap::{CMD_DATA_CONNECTION_READY, encode_sdu};

    #[test]
    fn acknowledgements_go_out_before_queued_data() {
        let mut core = Core::default();
        core.urgent.push_back(core.session.request());
        core.on_link_bytes(&encode_sdu(&[CMD_DATA_CONNECTION_READY]));
        core.urgent.clear();
        let mut frame = 100_000u32.to_be_bytes().to_vec();
        frame.extend(vec![0u8; 100_000]);
        core.on_stream_bytes(&frame);
        assert!(core.outbox.len() > 4);
        core.on_link_bytes(&encode_sdu(&data_message(b"abc")));
        core.refill();
        assert!(core.to_link.ends_with(&[0x10, 3]), "{:02x?}", core.to_link);
    }
}
