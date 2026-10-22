//! A whole Quick Share session through `send_over_gatt`, against our own receiver side
//! (`ble_receiver::bridge::BleBridge`) standing in for the phone.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use super::bridge::BridgeExit;
use super::gatt_bridge::GattLink;
use super::send_over_gatt;
use crate::channel::{self, ChannelMessage, TransferAction};
use crate::hdl::ble_receiver::bridge::{BleBridge, GattWrite, packet_limit};
use crate::hdl::{InboundRequest, MigratableStream, OutboundPayload, TransferState};
use crate::manager::drive_inbound;
use crate::upgrade::WifiLink;
use crate::upgrade::tests::{FakeLink, SERIAL};

const SMALL_FILE: usize = 100 * 1024;
const BIG_FILE: usize = 3 * 1024 * 1024;

struct Outcome {
    sent: Vec<u8>,
    received: Vec<u8>,
    /// Bytes written to ToPeripheral.
    up: usize,
    max_write: usize,
    exit: BridgeExit,
}

/// Runs a send session over a GATT link with the given ATT `mtu`. The phone's characteristics
/// are plain channels: each write is delivered in order, like an ATT write with response.
async fn try_session(link: Option<Arc<FakeLink>>, file_size: usize, mtu: u16) -> Option<Outcome> {
    let _guard = SERIAL.lock().await;
    let dyn_link: Option<Arc<dyn WifiLink>> = link.map(|l| l as Arc<dyn WifiLink>);
    *crate::WIFI_LINK.write().unwrap() = dyn_link;

    let dir = std::env::temp_dir().join(format!("rqs_gatt_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (src_dir, dst_dir) = (dir.join("src"), dir.join("dst"));
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&dst_dir).unwrap();
    *crate::CUSTOM_DOWNLOAD.write().unwrap() = Some(dst_dir.clone());

    let data: Vec<u8> = (0..file_size).map(|i| (i * 31 % 251) as u8).collect();
    let src = src_dir.join("payload.bin");
    std::fs::write(&src, &data).unwrap();

    // Phone: InboundRequest <-> BleBridge <-> channels <-> GattLink.
    let (inbound_end, bridge_end) = tokio::io::duplex(64 * 1024);
    let (phone_writes, phone_writes_rx) = mpsc::channel::<GattWrite>(16);
    let (notify_tx, notifications) = mpsc::channel(64);
    let phone = tokio::spawn(BleBridge::new(phone_writes_rx, notify_tx, bridge_end).run());
    let (rx_bus, mut rx_events) = broadcast::channel::<ChannelMessage>(256);
    let inbound = InboundRequest::new_over(
        MigratableStream::Duplex(inbound_end),
        "gatt-rx".into(),
        rx_bus.clone(),
    );
    let rx_task = tokio::spawn(drive_inbound(inbound, rx_bus.clone()));

    let (writes, mut writes_rx) = mpsc::channel::<Vec<u8>>(1);
    let (up, max_write) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (up_stat, max_stat) = (up.clone(), max_write.clone());
    let wire = tokio::spawn(async move {
        while let Some(data) = writes_rx.recv().await {
            up_stat.fetch_add(data.len(), Ordering::SeqCst);
            max_stat.fetch_max(data.len(), Ordering::SeqCst);
            if phone_writes.send(GattWrite { data, mtu }).await.is_err() {
                break;
            }
        }
    });
    let link = GattLink {
        writes,
        notifications,
        max_packet: packet_limit(mtu),
    };

    let (tx_bus, _tx_events) = broadcast::channel::<ChannelMessage>(256);
    let ctk = CancellationToken::new();
    let send = tokio::spawn(send_over_gatt(
        link,
        *b"SEND",
        "gatt-tx".into(),
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
        .expect("send_over_gatt did not return")
        .unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(20), rx_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(20), phone).await;
    let _ = tokio::time::timeout(Duration::from_secs(20), wire).await;
    ctk.cancel();

    let received = std::fs::read(dst_dir.join("payload.bin")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    *crate::WIFI_LINK.write().unwrap() = None;
    finished.then(|| Outcome {
        sent: data,
        received,
        up: up.load(Ordering::SeqCst),
        max_write: max_write.load(Ordering::SeqCst),
        exit: exit.expect("send_over_gatt failed"),
    })
}

/// The lib mis-encodes public keys with a leading zero byte in about 1 in 70 sessions
/// (see upgrade/tests.rs); such a session is repeated.
async fn session(link: Option<Arc<FakeLink>>, file_size: usize, mtu: u16) -> Outcome {
    for _ in 0..5 {
        if let Some(outcome) = try_session(link.clone(), file_size, mtu).await {
            return outcome;
        }
    }
    panic!("session kept aborting");
}

/// Once the transfer is done either side may close first.
fn closed_normally(exit: &BridgeExit) -> bool {
    matches!(
        exit,
        BridgeExit::PhoneDisconnected | BridgeExit::StreamClosed | BridgeExit::LinkClosed
    )
}

async fn plain_session(file_size: usize, mtu: u16) {
    let out = session(None, file_size, mtu).await;
    assert_eq!(out.sent, out.received);
    assert!(out.up >= file_size, "payload must cross the link");
    assert!(out.max_write <= packet_limit(mtu));
    assert!(closed_normally(&out.exit), "{:?}", out.exit);
}

#[tokio::test(flavor = "multi_thread")]
async fn small_file_with_509_byte_packets() {
    plain_session(SMALL_FILE, 512).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn small_file_with_20_byte_packets() {
    plain_session(SMALL_FILE, 23).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn hotspot_upgrade_after_gatt() {
    let link = Arc::new(FakeLink::default());
    let out = session(Some(link.clone()), BIG_FILE, 512).await;
    assert_eq!(out.sent, out.received);
    assert_eq!(link.hosts.load(Ordering::SeqCst), 1);
    assert_eq!(link.joins.load(Ordering::SeqCst), 1);
    // Only the handshake and the hand-over crossed the GATT link, not the file.
    assert!(out.up < BIG_FILE / 4, "{} bytes up", out.up);
    assert!(closed_normally(&out.exit), "{:?}", out.exit);
}

/// A phone that answers the weave request with an error packet: the failure has to reach the
/// bus (or the caller) at once, not only the app's watchdog.
#[tokio::test]
async fn a_weave_error_from_the_phone_is_reported() {
    let _guard = SERIAL.lock().await;
    *crate::WIFI_LINK.write().unwrap() = None;
    let path = std::env::temp_dir().join(format!("rqs_gatt_err_{}.txt", std::process::id()));
    std::fs::write(&path, b"hello").unwrap();

    let (writes, mut writes_rx) = mpsc::channel::<Vec<u8>>(8);
    let (notify_tx, notifications) = mpsc::channel(8);
    let link = GattLink {
        writes,
        notifications,
        max_packet: 509,
    };
    // The phone reads the connection request and answers with a weave ERROR (`0x82`).
    tokio::spawn(async move {
        let _request = writes_rx.recv().await;
        let _ = notify_tx.send(vec![0x82]).await;
        // Keep the writes side open and drain it, like a link that stays up.
        while writes_rx.recv().await.is_some() {}
    });

    let (bus, mut events) = broadcast::channel::<ChannelMessage>(64);
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        send_over_gatt(
            link,
            *b"TEST",
            "gatt-err".into(),
            "Phone".into(),
            OutboundPayload::Files(vec![path.to_string_lossy().into_owned()]),
            bus,
            CancellationToken::new(),
        ),
    )
    .await
    .expect("the send did not end");
    let _ = std::fs::remove_file(&path);

    let mut disconnected = false;
    while let Ok(message) = events.try_recv() {
        if let channel::Message::Client(c) = message.msg
            && c.state == Some(TransferState::Disconnected)
        {
            disconnected = true;
        }
    }
    assert!(
        disconnected || result.is_err(),
        "neither a Disconnected event nor an error: {result:?}"
    );
}
