//! Hardware-free bridge between GATT packets and the byte stream an `InboundRequest` reads.
//!
//! Phone to us: GATT writes go through the [`WeaveSession`]; `Data` goes to the stream.
//! Us to phone: the stream carries `[4-byte BE len][OfflineFrame]`; every complete frame
//! becomes one socket message, fragmented into packets for the notify characteristic.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};

use super::session::{Event, WeaveSession};
use super::weave::MAX_MESSAGE_SIZE;

/// The phone has to send its connection request this soon after subscribing.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// After the handshake, the phone has to send its first OfflineFrame within this time of its
/// last write; a connected but silent peer would otherwise hold the one session slot.
pub const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a closing bridge keeps handing over what it already holds.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);
/// Bytes from the phone waiting for the session to read them.
const MAX_PENDING_TO_STREAM: usize = 1 << 20;
/// Notify packets queued before the stream is no longer read (BLE is the slow side).
const MAX_OUTBOX: usize = 1024;
const READ_CHUNK: usize = 16 * 1024;
/// Largest frame accepted from the session: the weave message limit minus hash and length.
const MAX_FRAME: usize = MAX_MESSAGE_SIZE - 7;
/// ATT header of a notification.
const ATT_OVERHEAD: usize = 3;

/// One write to the ToPeripheral characteristic.
#[derive(Debug, Clone)]
pub struct GattWrite {
    pub data: Vec<u8>,
    /// Exchanged ATT MTU of the link the write came in on.
    pub mtu: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeExit {
    /// The phone closed the socket.
    PhoneDisconnected,
    /// The session closed or dropped its end (also what a finished upgrade looks like).
    StreamClosed,
    /// No connection request in time.
    ConnectTimeout,
    /// Connected, but no data from the phone in time.
    SilentPeer,
    /// The phone or the session broke the protocol.
    Protocol(String),
    /// The GATT side went away (link lost, notifications stopped, session replaced).
    GattClosed,
}

/// Largest weave packet that fits in one notification at `mtu`.
pub fn packet_limit(mtu: u16) -> usize {
    (mtu as usize).saturating_sub(ATT_OVERHEAD)
}

#[derive(Debug)]
pub struct BleBridge {
    writes: mpsc::Receiver<GattWrite>,
    notify: mpsc::Sender<Vec<u8>>,
    stream: DuplexStream,
    connect_timeout: Duration,
    first_frame_timeout: Duration,
}

impl BleBridge {
    /// `notify` receives the packets to send on FromPeripheral, in order. Dropping the bridge
    /// drops `stream`, which the session sees as end of stream.
    pub fn new(
        writes: mpsc::Receiver<GattWrite>,
        notify: mpsc::Sender<Vec<u8>>,
        stream: DuplexStream,
    ) -> Self {
        Self {
            writes,
            notify,
            stream,
            connect_timeout: CONNECT_TIMEOUT,
            first_frame_timeout: FIRST_FRAME_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    #[cfg(test)]
    pub fn with_first_frame_timeout(mut self, timeout: Duration) -> Self {
        self.first_frame_timeout = timeout;
        self
    }

    /// Runs until the session is over. Packets already produced are handed to `notify`
    /// first, so the phone gets the last frame (e.g. the plaintext DISCONNECTION).
    pub async fn run(self) -> BridgeExit {
        let Self {
            mut writes,
            notify,
            stream,
            connect_timeout,
            first_frame_timeout,
        } = self;
        let (mut reader, mut writer) = tokio::io::split(stream);
        let mut core = Core::default();
        let mut chunk = vec![0u8; READ_CHUNK];
        let connect_at = Instant::now() + connect_timeout;
        let mut silent_at = connect_at;
        let mut flush_at: Option<Instant> = None;

        // Writing to the stream is one branch of the loop, not an await inside another one:
        // the session may be blocked writing to us while we would be blocked writing to it.
        while !core.is_done() {
            if core.exit.is_some() && flush_at.is_none() {
                flush_at = Some(Instant::now() + FLUSH_TIMEOUT);
            }
            tokio::select! {
                write = writes.recv(),
                    if core.exit.is_none() && core.to_stream.len() < MAX_PENDING_TO_STREAM =>
                {
                    match write {
                        Some(write) => {
                            silent_at = Instant::now() + first_frame_timeout;
                            core.on_write(write);
                        }
                        None => core.end(BridgeExit::GattClosed),
                    }
                }
                read = reader.read(&mut chunk),
                    if core.exit.is_none() && core.outbox.len() < MAX_OUTBOX =>
                {
                    match read {
                        Ok(n) if n > 0 => core.on_stream_bytes(&chunk[..n]),
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
                permit = notify.reserve(), if !core.outbox.is_empty() => {
                    match (permit, core.outbox.pop_front()) {
                        (Ok(permit), Some(packet)) => permit.send(packet),
                        (Err(_), _) => return BridgeExit::GattClosed,
                        (Ok(_), None) => {}
                    }
                }
                () = sleep_until(connect_at), if !core.connected && core.exit.is_none() => {
                    core.end(BridgeExit::ConnectTimeout);
                }
                () = sleep_until(silent_at),
                    if core.connected && !core.got_data && core.exit.is_none() =>
                {
                    core.end(BridgeExit::SilentPeer);
                }
                () = sleep_until(flush_at.unwrap_or(connect_at)), if flush_at.is_some() => {
                    break;
                }
            }
        }
        core.exit.unwrap_or(BridgeExit::StreamClosed)
    }
}

/// The synchronous part of the bridge: no I/O, so every transition can be tested directly.
#[derive(Debug, Default)]
struct Core {
    session: WeaveSession,
    connected: bool,
    /// The phone has sent at least one data packet.
    got_data: bool,
    /// Phone data not yet written to the stream.
    to_stream: Vec<u8>,
    /// Partial frame read from the stream.
    from_stream: Vec<u8>,
    /// Packets not yet handed to the notify channel.
    outbox: VecDeque<Vec<u8>>,
    exit: Option<BridgeExit>,
}

impl Core {
    fn is_done(&self) -> bool {
        self.exit.is_some() && self.outbox.is_empty() && self.to_stream.is_empty()
    }

    /// The first reason wins. What is still worth delivering depends on who ended it.
    fn end(&mut self, exit: BridgeExit) {
        if self.exit.is_some() {
            return;
        }
        match exit {
            // Let the session read the rest of what the phone sent.
            BridgeExit::PhoneDisconnected => {}
            // The session is gone, but the phone still gets its last frames.
            BridgeExit::StreamClosed => self.to_stream.clear(),
            _ => {
                self.to_stream.clear();
                self.outbox.clear();
            }
        }
        self.exit = Some(exit);
    }

    fn on_write(&mut self, write: GattWrite) {
        if !self.connected {
            self.session.set_packet_limit(packet_limit(write.mtu));
        }
        for event in self.session.feed_write(&write.data) {
            match event {
                Event::Send(packet) => self.outbox.push_back(packet),
                Event::Connected { packet_size } => {
                    debug!("ble bridge: weave connected, packet size {packet_size}");
                    self.connected = true;
                }
                Event::Introduction => debug!("ble bridge: socket introduction"),
                Event::Data(data) => {
                    self.got_data = true;
                    self.to_stream.extend_from_slice(&data);
                }
                Event::Disconnected => self.end(BridgeExit::PhoneDisconnected),
                Event::Failed(reason) => {
                    warn!("ble bridge: {reason}");
                    self.end(BridgeExit::Protocol(reason));
                }
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
            // Before the handshake there is nowhere to send to; the session never writes then.
            self.outbox.extend(self.session.queue_data(&frame));
        }
    }
}

#[cfg(test)]
mod tests;
