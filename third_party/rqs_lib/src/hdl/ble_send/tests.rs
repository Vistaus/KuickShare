use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::bridge::{BridgeExit, CentralBridge};
use super::l2cap::{
    CMD_DATA_CONNECTION_FAILURE, CMD_DATA_CONNECTION_READY, CMD_REQUEST_DATA_CONNECTION,
    MAX_SEND_SDU, SduDecoder, disconnection_message, encode_sdu, introduction_message, parse_ack,
};
use super::send_over_l2cap;
use crate::channel::{self, ChannelMessage, TransferAction};
use crate::hdl::ble_receiver::socket::{SERVICE_HASH, data_message};
use crate::hdl::{InboundRequest, MigratableStream, OutboundPayload, TransferState};
use crate::manager::drive_inbound;
use crate::upgrade::WifiLink;
use crate::upgrade::tests::{FakeLink, SERIAL};

const STEP: Duration = Duration::from_secs(5);

fn frame(body: &[u8]) -> Vec<u8> {
    let mut f = (body.len() as u32).to_be_bytes().to_vec();
    f.extend_from_slice(body);
    f
}

// --- Bridge against a hand-driven peripheral -----------------------------------------------

struct Harness {
    phone: DuplexStream,
    session_end: DuplexStream,
    bridge: JoinHandle<BridgeExit>,
}

impl Harness {
    fn start() -> Self {
        Self::start_with(CentralBridge::new)
    }

    fn start_with(
        make: impl FnOnce(DuplexStream, DuplexStream) -> CentralBridge<DuplexStream>,
    ) -> Self {
        let (phone, link) = tokio::io::duplex(4096);
        let (session_end, bridge_end) = tokio::io::duplex(64 * 1024);
        let bridge = tokio::spawn(make(link, bridge_end).run());
        Self {
            phone,
            session_end,
            bridge,
        }
    }

    async fn read_sdu(&mut self) -> Option<Vec<u8>> {
        let mut len = [0u8; 4];
        tokio::time::timeout(STEP, self.phone.read_exact(&mut len))
            .await
            .expect("no SDU")
            .ok()?;
        let mut sdu = vec![0u8; u32::from_be_bytes(len) as usize];
        self.phone.read_exact(&mut sdu).await.ok()?;
        Some(sdu)
    }

    async fn write_sdu(&mut self, payload: &[u8]) {
        self.phone.write_all(&encode_sdu(payload)).await.unwrap();
    }

    /// Answers the request like a phone and returns the INTRODUCTION it got.
    async fn accept(&mut self) -> Vec<u8> {
        assert_eq!(
            self.read_sdu().await.unwrap(),
            [CMD_REQUEST_DATA_CONNECTION]
        );
        self.write_sdu(&[CMD_DATA_CONNECTION_READY]).await;
        self.read_sdu().await.unwrap()
    }

    async fn exit(self) -> BridgeExit {
        tokio::time::timeout(STEP, self.bridge)
            .await
            .expect("bridge did not stop")
            .unwrap()
    }
}

async fn read_all(stream: &mut DuplexStream) -> Vec<u8> {
    let mut out = Vec::new();
    tokio::time::timeout(STEP, stream.read_to_end(&mut out))
        .await
        .expect("stream was not closed")
        .unwrap();
    out
}

#[tokio::test]
async fn handshake_then_introduction() {
    let mut h = Harness::start();
    assert_eq!(h.accept().await, introduction_message());
}

#[tokio::test]
async fn the_stream_is_not_read_before_the_handshake_is_done() {
    let mut h = Harness::start();
    let early = frame(b"early");
    h.session_end.write_all(&early).await.unwrap();
    assert_eq!(h.read_sdu().await.unwrap(), [CMD_REQUEST_DATA_CONNECTION]);
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.write_sdu(&[CMD_DATA_CONNECTION_READY]).await;
    assert_eq!(h.read_sdu().await.unwrap(), introduction_message());
    assert_eq!(h.read_sdu().await.unwrap(), data_message(&early));
}

#[tokio::test]
async fn stream_frames_become_one_message_each_however_they_are_chunked() {
    let mut h = Harness::start();
    h.accept().await;
    let frames: Vec<Vec<u8>> = [10usize, 1200, 3]
        .iter()
        .map(|n| frame(&vec![7u8; *n]))
        .collect();
    let joined: Vec<u8> = frames.concat();
    // Cuts inside a length prefix, inside a body, and two frames at once.
    for part in [&joined[..2], &joined[2..700], &joined[700..]] {
        h.session_end.write_all(part).await.unwrap();
        h.session_end.flush().await.unwrap();
    }
    for expected in &frames {
        assert_eq!(h.read_sdu().await.unwrap(), data_message(expected));
    }
}

#[tokio::test]
async fn big_frames_are_split_into_bounded_sdus() {
    let mut h = Harness::start();
    h.accept().await;
    let big = frame(&(0..100_000u32).map(|i| i as u8).collect::<Vec<_>>());
    h.session_end.write_all(&big).await.unwrap();
    let mut got = Vec::new();
    while got.len() < big.len() {
        let sdu = h.read_sdu().await.unwrap();
        assert!(sdu.len() <= MAX_SEND_SDU);
        assert_eq!(sdu[..3], SERVICE_HASH);
        got.extend_from_slice(&sdu[3..]);
    }
    assert_eq!(got, big);
}

#[tokio::test]
async fn phone_data_is_acknowledged_in_order_and_delivered() {
    let mut h = Harness::start();
    h.accept().await;
    // Two messages in one write, then one cut in half.
    let (a, b, c) = (vec![1u8; 300], vec![2u8; 1024], vec![3u8; 5]);
    let mut wire = encode_sdu(&data_message(&a));
    wire.extend(encode_sdu(&data_message(&b)));
    let last = encode_sdu(&data_message(&c));
    wire.extend_from_slice(&last[..3]);
    h.phone.write_all(&wire).await.unwrap();
    h.phone.write_all(&last[3..]).await.unwrap();
    for n in [300, 1024, 5] {
        assert_eq!(parse_ack(&h.read_sdu().await.unwrap()), Some(n));
    }
    let mut got = vec![0u8; a.len() + b.len() + c.len()];
    h.session_end.read_exact(&mut got).await.unwrap();
    assert_eq!(got, [a, b, c].concat());
}

#[tokio::test]
async fn failure_byte_means_refused_and_closes_the_stream() {
    let mut h = Harness::start();
    assert_eq!(h.read_sdu().await.unwrap(), [CMD_REQUEST_DATA_CONNECTION]);
    h.write_sdu(&[CMD_DATA_CONNECTION_FAILURE]).await;
    assert!(read_all(&mut h.session_end).await.is_empty());
    assert_eq!(h.exit().await, BridgeExit::Refused);
}

#[tokio::test]
async fn no_answer_times_out() {
    let h = Harness::start_with(|l, s| {
        CentralBridge::new(l, s).with_handshake_timeout(Duration::from_millis(50))
    });
    assert_eq!(h.exit().await, BridgeExit::HandshakeTimeout);
}

#[tokio::test]
async fn garbage_ends_the_session() {
    let mut h = Harness::start();
    h.write_sdu(&[0x42]).await;
    assert!(matches!(h.exit().await, BridgeExit::Protocol(_)));

    let mut h = Harness::start();
    h.accept().await;
    h.phone.write_all(&[0xff; 8]).await.unwrap();
    assert!(read_all(&mut h.session_end).await.is_empty());
    assert!(matches!(h.exit().await, BridgeExit::Protocol(_)));
}

#[tokio::test]
async fn a_bogus_frame_length_from_the_session_ends_it() {
    let mut h = Harness::start();
    h.accept().await;
    h.session_end.write_all(&[0xff; 4]).await.unwrap();
    assert!(matches!(h.exit().await, BridgeExit::Protocol(_)));
}

#[tokio::test]
async fn a_silent_connected_peer_times_out_but_only_before_its_first_data() {
    let mut h = Harness::start_with(|l, s| {
        CentralBridge::new(l, s).with_first_data_timeout(Duration::from_millis(50))
    });
    h.accept().await;
    assert_eq!(h.exit().await, BridgeExit::SilentPeer);

    let mut h = Harness::start_with(|l, s| {
        CentralBridge::new(l, s).with_first_data_timeout(Duration::from_millis(100))
    });
    h.accept().await;
    h.phone
        .write_all(&encode_sdu(&data_message(b"hi")))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(!h.bridge.is_finished());
}

#[tokio::test]
async fn peer_disconnection_delivers_earlier_data_first() {
    let mut h = Harness::start();
    h.accept().await;
    let payload = vec![4u8; 3000];
    for chunk in payload.chunks(1000) {
        h.write_sdu(&data_message(chunk)).await;
    }
    h.write_sdu(&disconnection_message()).await;
    assert_eq!(read_all(&mut h.session_end).await, payload);
    assert_eq!(h.exit().await, BridgeExit::PhoneDisconnected);
}

#[tokio::test]
async fn closing_the_stream_delivers_the_last_frame_then_disconnects() {
    let mut h = Harness::start();
    h.accept().await;
    let last = frame(&[9u8; 20_000]);
    h.session_end.write_all(&last).await.unwrap();
    h.session_end.shutdown().await.unwrap();

    let mut got = Vec::new();
    loop {
        let sdu = h.read_sdu().await.unwrap();
        if sdu == disconnection_message() {
            break;
        }
        got.extend_from_slice(&sdu[3..]);
    }
    assert_eq!(got, last);
    assert!(h.read_sdu().await.is_none(), "the link must be closed");
    assert_eq!(h.exit().await, BridgeExit::StreamClosed);
}

#[tokio::test]
async fn losing_the_link_delivers_what_arrived_and_ends_the_session() {
    let mut h = Harness::start();
    h.accept().await;
    h.write_sdu(&data_message(b"last words")).await;
    h.phone.shutdown().await.unwrap();
    assert_eq!(read_all(&mut h.session_end).await, b"last words");
    assert_eq!(h.exit().await, BridgeExit::LinkClosed);

    // A write error on the link.
    let mut h = Harness::start();
    h.accept().await;
    let Harness {
        phone,
        mut session_end,
        bridge,
    } = h;
    drop(phone);
    session_end.write_all(&frame(&[1u8; 64])).await.unwrap();
    assert_eq!(
        tokio::time::timeout(STEP, bridge).await.unwrap().unwrap(),
        BridgeExit::LinkClosed
    );
}

#[tokio::test]
async fn a_stalled_peer_cannot_deadlock_the_bridge() {
    // Both sides send a lot before they read.
    let mut h = Harness::start();
    h.accept().await;
    let (mut s_rd, mut s_wr) = tokio::io::split(std::mem::replace(
        &mut h.session_end,
        tokio::io::duplex(1).0,
    ));
    let session = tokio::spawn(async move {
        s_wr.write_all(&frame(&vec![5u8; 200 * 1024]))
            .await
            .unwrap();
        let mut got = vec![0u8; 300 * 1024];
        s_rd.read_exact(&mut got).await.unwrap();
        got
    });
    let (mut p_rd, mut p_wr) = tokio::io::split(h.phone);
    let payload = vec![6u8; 300 * 1024];
    let send = payload.clone();
    let writer = tokio::spawn(async move {
        for chunk in send.chunks(1024) {
            p_wr.write_all(&encode_sdu(&data_message(chunk)))
                .await
                .unwrap();
        }
        p_wr
    });
    let mut dec = SduDecoder::default();
    let (mut from_central, mut buf) = (0usize, vec![0u8; 8192]);
    while from_central < 200 * 1024 + 4 {
        let n = tokio::time::timeout(Duration::from_secs(10), p_rd.read(&mut buf))
            .await
            .expect("deadlock")
            .unwrap();
        for sdu in dec.feed(&buf[..n]).unwrap() {
            if sdu[..3] == SERVICE_HASH {
                from_central += sdu.len() - 3;
            }
        }
    }
    let got = tokio::time::timeout(Duration::from_secs(10), session)
        .await
        .expect("deadlock")
        .unwrap();
    assert_eq!(got, payload);
    let _ = writer.await;
}

// --- A whole Quick Share session through L2CAP, against our own receiver -------------------

const PHONE_SDU: usize = 1024;
/// The phone stops sending until its data is acknowledged: like a real one, and it makes
/// the tests depend on the acknowledgements.
const PHONE_WINDOW: usize = 1024;

#[derive(Default)]
struct PhoneStats {
    /// Bytes read from the link (central to phone).
    up: AtomicUsize,
    /// Bytes written to the link.
    down: AtomicUsize,
    sent: AtomicUsize,
    acked: AtomicUsize,
    introduced: AtomicBool,
    said_goodbye: AtomicBool,
    max_sdu: AtomicUsize,
}

/// The phone's side of L2CAP: answers the request, sends its INTRODUCTION, acknowledges
/// nothing (it only receives), forwards data to and from the receiver stack on `stack`.
fn spawn_phone(link: DuplexStream, stack: DuplexStream, stats: Arc<PhoneStats>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let (mut link_rd, mut link_wr) = tokio::io::split(link);
        let (mut stack_rd, mut stack_wr) = tokio::io::split(stack);
        let mut dec = SduDecoder::default();
        let (mut rbuf, mut chunk) = (vec![0u8; 8192], vec![0u8; PHONE_SDU]);
        let mut unacked = 0usize;
        let write = |bytes: Vec<u8>| {
            stats.down.fetch_add(bytes.len(), Ordering::SeqCst);
            bytes
        };
        'run: loop {
            tokio::select! {
                n = link_rd.read(&mut rbuf) => {
                    let Ok(n @ 1..) = n else { break };
                    stats.up.fetch_add(n, Ordering::SeqCst);
                    for sdu in dec.feed(&rbuf[..n]).expect("central sent bad framing") {
                        stats.max_sdu.fetch_max(sdu.len(), Ordering::SeqCst);
                        if sdu == [CMD_REQUEST_DATA_CONNECTION] {
                            let mut out = encode_sdu(&[CMD_DATA_CONNECTION_READY]);
                            out.extend(encode_sdu(&introduction_message()));
                            if link_wr.write_all(&write(out)).await.is_err() {
                                break 'run;
                            }
                        } else if sdu[..3] == SERVICE_HASH {
                            if stack_wr.write_all(&sdu[3..]).await.is_err() {
                                break 'run;
                            }
                        } else if sdu == disconnection_message() {
                            stats.said_goodbye.store(true, Ordering::SeqCst);
                            break 'run;
                        } else if sdu == introduction_message() {
                            stats.introduced.store(true, Ordering::SeqCst);
                        } else {
                            let acked = parse_ack(&sdu).expect("unknown control message");
                            assert!(acked <= unacked, "acknowledged more than was sent");
                            unacked -= acked;
                            stats.acked.fetch_add(acked, Ordering::SeqCst);
                        }
                    }
                }
                n = stack_rd.read(&mut chunk), if unacked < PHONE_WINDOW => {
                    let Ok(n @ 1..) = n else {
                        let bye = encode_sdu(&disconnection_message());
                        let _ = link_wr.write_all(&write(bye)).await;
                        break;
                    };
                    unacked += n;
                    stats.sent.fetch_add(n, Ordering::SeqCst);
                    let sdu = encode_sdu(&data_message(&chunk[..n]));
                    if link_wr.write_all(&write(sdu)).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = stack_wr.shutdown().await;
        let _ = link_wr.shutdown().await;
    })
}

struct Outcome {
    sent: Vec<u8>,
    received: Vec<u8>,
    stats: Arc<PhoneStats>,
    exit: BridgeExit,
}

const SMALL_FILE: usize = 100 * 1024;
const BIG_FILE: usize = 3 * 1024 * 1024;

async fn try_session(
    link: Option<Arc<FakeLink>>,
    file_size: usize,
    link_buffer: usize,
) -> Option<Outcome> {
    let _guard = SERIAL.lock().await;
    let dyn_link: Option<Arc<dyn WifiLink>> = link.map(|l| l as Arc<dyn WifiLink>);
    *crate::WIFI_LINK.write().unwrap() = dyn_link;

    let dir = std::env::temp_dir().join(format!("rqs_l2cap_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (src_dir, dst_dir) = (dir.join("src"), dir.join("dst"));
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&dst_dir).unwrap();
    *crate::CUSTOM_DOWNLOAD.write().unwrap() = Some(dst_dir.clone());

    let data: Vec<u8> = (0..file_size).map(|i| (i * 31 % 251) as u8).collect();
    let src = src_dir.join("payload.bin");
    std::fs::write(&src, &data).unwrap();

    // Receiver: InboundRequest <-> phone <-> L2CAP link <-> send_over_l2cap.
    let (rx_end, phone_stack) = tokio::io::duplex(64 * 1024);
    let (link_central, link_phone) = tokio::io::duplex(link_buffer);
    let stats = Arc::new(PhoneStats::default());
    let phone = spawn_phone(link_phone, phone_stack, stats.clone());
    let (rx_bus, mut rx_events) = broadcast::channel::<ChannelMessage>(256);
    let inbound = InboundRequest::new_over(
        MigratableStream::Duplex(rx_end),
        "l2cap-rx".into(),
        rx_bus.clone(),
    );
    let rx_task = tokio::spawn(drive_inbound(inbound, rx_bus.clone()));

    let (tx_bus, _tx_events) = broadcast::channel::<ChannelMessage>(256);
    let ctk = CancellationToken::new();
    let send = tokio::spawn(send_over_l2cap(
        link_central,
        *b"SEND",
        "l2cap-tx".into(),
        "test".into(),
        OutboundPayload::Files(vec![src.to_string_lossy().into_owned()]),
        tx_bus,
        ctk.clone(),
    ));

    let run = async {
        loop {
            let msg = rx_events.recv().await.unwrap();
            let Some(client) = msg.msg.as_client() else {
                continue;
            };
            match client.state {
                Some(TransferState::WaitingForUserConsent) => {
                    rx_bus
                        .send(ChannelMessage {
                            id: msg.id.clone(),
                            msg: channel::Message::Lib {
                                action: TransferAction::ConsentAccept,
                            },
                        })
                        .unwrap();
                }
                Some(TransferState::Finished) => break true,
                Some(TransferState::Disconnected | TransferState::Cancelled) => break false,
                _ => {}
            }
        }
    };
    let finished = tokio::time::timeout(Duration::from_secs(120), run)
        .await
        .expect("transfer did not finish");
    let exit = tokio::time::timeout(Duration::from_secs(20), send)
        .await
        .expect("send_over_l2cap did not return")
        .unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(20), rx_task).await;
    tokio::time::timeout(Duration::from_secs(20), phone)
        .await
        .expect("phone did not stop")
        .unwrap();
    ctk.cancel();

    let received = std::fs::read(dst_dir.join("payload.bin")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    *crate::WIFI_LINK.write().unwrap() = None;
    if !finished {
        return None;
    }
    Some(Outcome {
        sent: data,
        received,
        stats,
        exit: exit.expect("send_over_l2cap failed"),
    })
}

/// The lib mis-encodes public keys with a leading zero byte in about 1 in 70 sessions
/// (see upgrade/tests.rs); such a session is repeated.
async fn session(link: Option<Arc<FakeLink>>, file_size: usize, link_buffer: usize) -> Outcome {
    for _ in 0..5 {
        if let Some(outcome) = try_session(link.clone(), file_size, link_buffer).await {
            return outcome;
        }
    }
    panic!("session kept aborting");
}

/// Once the transfer is done either side may close first; a write to a link the phone has
/// just closed ends as `LinkClosed`.
fn closed_normally(exit: &BridgeExit) -> bool {
    matches!(
        exit,
        BridgeExit::PhoneDisconnected | BridgeExit::StreamClosed | BridgeExit::LinkClosed
    )
}

async fn plain_session(file_size: usize, link_buffer: usize) {
    let out = session(None, file_size, link_buffer).await;
    assert_eq!(out.sent, out.received);
    let s = &out.stats;
    assert!(s.introduced.load(Ordering::SeqCst));
    assert!(
        s.up.load(Ordering::SeqCst) >= file_size,
        "payload must cross the link"
    );
    assert!(s.max_sdu.load(Ordering::SeqCst) <= MAX_SEND_SDU);
    assert!(s.sent.load(Ordering::SeqCst) > 0);
    assert!(s.acked.load(Ordering::SeqCst) <= s.sent.load(Ordering::SeqCst));
    assert!(closed_normally(&out.exit), "{:?}", out.exit);
}

async fn upgraded_session(file_size: usize, link_buffer: usize) {
    let link = Arc::new(FakeLink::default());
    let out = session(Some(link.clone()), file_size, link_buffer).await;
    assert_eq!(out.sent, out.received);
    assert_eq!(link.hosts.load(Ordering::SeqCst), 1);
    assert_eq!(link.joins.load(Ordering::SeqCst), 1);
    // Only the handshake and the hand-over crossed the L2CAP link, not the file.
    let (up, down) = (
        out.stats.up.load(Ordering::SeqCst),
        out.stats.down.load(Ordering::SeqCst),
    );
    assert!(up < file_size / 4, "{up} bytes up");
    assert!(down < 64 * 1024, "{down} bytes down");
    assert!(closed_normally(&out.exit), "{:?}", out.exit);
}

#[tokio::test(flavor = "multi_thread")]
async fn small_file_over_l2cap() {
    plain_session(SMALL_FILE, 64 * 1024).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn small_file_over_a_tiny_link_buffer() {
    plain_session(SMALL_FILE, 700).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn hotspot_upgrade_after_l2cap() {
    upgraded_session(BIG_FILE, 4096).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_hotspot_falls_back_to_l2cap() {
    let link = Arc::new(FakeLink {
        join_fails: true,
        ..Default::default()
    });
    let out = session(Some(link), SMALL_FILE, 4096).await;
    assert_eq!(out.sent, out.received);
    assert!(out.stats.up.load(Ordering::SeqCst) >= SMALL_FILE);
    assert!(closed_normally(&out.exit), "{:?}", out.exit);
}

#[tokio::test]
async fn a_refusing_phone_is_an_error() {
    let (central, mut phone) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        let mut buf = [0u8; 5];
        phone.read_exact(&mut buf).await.unwrap();
        phone
            .write_all(&encode_sdu(&[CMD_DATA_CONNECTION_FAILURE]))
            .await
            .unwrap();
        phone
    });
    let (bus, _rx) = broadcast::channel(16);
    let result = send_over_l2cap(
        central,
        *b"SEND",
        "x".into(),
        "test".into(),
        OutboundPayload::Files(vec![]),
        bus,
        CancellationToken::new(),
    )
    .await;
    assert!(result.is_err());
}
