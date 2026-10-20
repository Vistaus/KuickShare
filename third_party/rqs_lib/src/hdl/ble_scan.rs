//! Windowed BLE scan for phones in Receive mode (Everyone visibility), found by their 0xFEF3
//! service data (see `ble_receiver::service_data`), and the send to one of them.
//!
//! [`BleScanner::scan`] runs one [`scan_window`] per user action (discovery start, refresh)
//! and turns each [`ScanEvent`] into an `EndpointInfo` with id `ble:<endpoint_id>` and
//! `present` set from `Found`/`Gone`. One [`Dedup`] lives as long as the process, so devices
//! that vanish between two windows are reported. [`BleScanner::send`] scans again for the
//! target (its address and PSM rotate at every scan), dials it and runs the send session.
//!
//! The scan must NEVER run continuously: a permanent BlueZ discovery blocks the reconnection
//! of bonded LE devices (rquickshare #429). Hence a bounded window, a minimum interval, and
//! one lock shared with the send: no scan while a dial or a transfer is going on.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use bluer::{Adapter, AdapterEvent, Address, AddressType, DiscoveryFilter, DiscoveryTransport};
use bluer::{Uuid, UuidExt};
use futures::StreamExt;
use once_cell::sync::Lazy;
use tokio::sync::{MutexGuard, broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use super::ble_receiver::service_data::{ReceiverAdvertisement, SERVICE_UUID};
use super::ble_send::dial::dial_l2cap;
use super::ble_send::send_over_l2cap;
use super::{BleTarget, EndpointInfo, OutboundPayload};
use crate::DeviceType;
use crate::channel::ChannelMessage;

/// Long enough to catch a phone that advertises every second or two, short enough for a
/// refresh button. The window is never cut short: a phone can show up at any moment of it.
pub const SCAN_WINDOW: Duration = Duration::from_secs(10);

/// Minimum time between the starts of two discovery windows.
pub const MIN_SCAN_INTERVAL: Duration = Duration::from_secs(4);

/// How long a send looks for its target again before giving up.
pub const RESCAN_WINDOW: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, PartialEq)]
pub struct ScanResult {
    /// Stable identity of the phone; the address rotates.
    pub endpoint_id: [u8; 4],
    pub name: String,
    pub device_type: DeviceType,
    pub address: Address,
    pub address_type: AddressType,
    pub psm: Option<u16>,
    pub rssi: Option<i16>,
}

impl ScanResult {
    /// Equal in everything a caller can act on; the RSSI jitters on every packet.
    fn same_target(&self, other: &Self) -> bool {
        Self {
            rssi: None,
            ..self.clone()
        } == Self {
            rssi: None,
            ..other.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScanEvent {
    /// First sighting of a phone, or a change of its address, name or PSM.
    Found(ScanResult),
    /// Seen in the previous window, not in the last one.
    Gone([u8; 4]),
}

/// Turns what BlueZ knows about a device into a result. `None` for anything that is not a
/// full 0xFEF3 advertisement of a receiver visible to everyone.
pub fn scan_result(
    address: Address,
    address_type: AddressType,
    rssi: Option<i16>,
    service_data: &[u8],
) -> Option<ScanResult> {
    let adv = ReceiverAdvertisement::parse(service_data)?;
    Some(ScanResult {
        endpoint_id: adv.endpoint_id,
        name: adv.name,
        device_type: DeviceType::from_raw_value(adv.device_type),
        address,
        address_type,
        psm: adv.psm,
        rssi,
    })
}

/// Keeps one entry per endpoint id (the newest sighting, hence the newest address wins) and
/// tracks which phones disappeared from one window to the next.
#[derive(Debug, Default)]
pub struct Dedup {
    /// Reported by the previous windows.
    known: HashMap<[u8; 4], ScanResult>,
    current: HashMap<[u8; 4], ScanResult>,
}

impl Dedup {
    /// Forgets what a cancelled window had seen.
    pub fn begin_window(&mut self) {
        self.current.clear();
    }

    /// Returns the result when the caller has to be told about it.
    pub fn observe(&mut self, result: ScanResult) -> Option<ScanResult> {
        let id = result.endpoint_id;
        let changed = self
            .current
            .get(&id)
            .or_else(|| self.known.get(&id))
            .is_none_or(|prev| !prev.same_target(&result));
        self.current.insert(id, result.clone());
        changed.then_some(result)
    }

    /// Ids seen in the previous windows but not in this one; the window becomes the baseline.
    pub fn end_window(&mut self) -> Vec<[u8; 4]> {
        let mut gone: Vec<_> = self
            .known
            .keys()
            .filter(|id| !self.current.contains_key(*id))
            .copied()
            .collect();
        gone.sort_unstable();
        self.known = std::mem::take(&mut self.current);
        gone
    }
}

/// Scans for `duration`, then reports the phones that are gone. The discovery is stopped on
/// every exit path, including cancellation of the future, by dropping the stream: bluer ends
/// the BlueZ `StartDiscovery` session when it sees the stream closed, a moment later.
pub async fn scan_window(
    adapter: &Adapter,
    duration: Duration,
    dedup: &mut Dedup,
    mut sink: impl FnMut(ScanEvent),
) -> anyhow::Result<()> {
    dedup.begin_window();
    adapter
        .set_discovery_filter(DiscoveryFilter {
            transport: DiscoveryTransport::Le,
            duplicate_data: true,
            ..Default::default()
        })
        .await
        .context("set the discovery filter")?;
    collect(adapter, duration, dedup, &mut sink).await?;
    for id in dedup.end_window() {
        sink(ScanEvent::Gone(id));
    }
    Ok(())
}

async fn collect(
    adapter: &Adapter,
    duration: Duration,
    dedup: &mut Dedup,
    sink: &mut impl FnMut(ScanEvent),
) -> anyhow::Result<()> {
    let stream = adapter
        .discover_devices_with_changes()
        .await
        .context("start the discovery")?;
    futures::pin_mut!(stream);
    let deadline = tokio::time::sleep(duration);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            () = &mut deadline => return Ok(()),
            event = stream.next() => match event {
                Some(AdapterEvent::DeviceAdded(address)) => {
                    if let Some(found) = read_device(adapter, address).await
                        && let Some(new) = dedup.observe(found)
                    {
                        sink(ScanEvent::Found(new));
                    }
                }
                Some(_) => {}
                None => bail!("the discovery ended before the window did"),
            },
        }
    }
}

/// BlueZ keeps devices it saw long ago: only one with an RSSI is on the air now.
async fn read_device(adapter: &Adapter, address: Address) -> Option<ScanResult> {
    let device = adapter.device(address).ok()?;
    let rssi = device.rssi().await.ok()??;
    let data = device.service_data().await.ok()??;
    let bytes = data.get(&Uuid::from_u16(SERVICE_UUID))?;
    let address_type = device.address_type().await.ok()?;
    scan_result(address, address_type, Some(rssi), bytes)
}

/// What the `EndpointInfo` bus is told about a scan event.
pub fn endpoint_info(event: &ScanEvent) -> EndpointInfo {
    let (target, name, rtype, present) = match event {
        ScanEvent::Found(r) => (
            BleTarget {
                endpoint_id: r.endpoint_id,
            },
            Some(r.name.clone()),
            Some(r.device_type.clone()),
            true,
        ),
        ScanEvent::Gone(id) => (BleTarget { endpoint_id: *id }, None, None, false),
    };
    let id = target.id();
    EndpointInfo {
        fullname: format!("{id}._ble"),
        id,
        name,
        ip: None,
        port: None,
        rtype,
        present: Some(present),
        ble: Some(target),
    }
}

/// The sighting a send waits for: its phone, with a PSM to dial.
fn target_found(event: ScanEvent, target: BleTarget) -> Option<ScanResult> {
    match event {
        ScanEvent::Found(r) if r.endpoint_id == target.endpoint_id && r.psm.is_some() => Some(r),
        _ => None,
    }
}

fn interval_elapsed(last_start: Option<Instant>, now: Instant) -> bool {
    last_start.is_none_or(|l| now.saturating_duration_since(l) >= MIN_SCAN_INTERVAL)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanOutcome {
    Done,
    /// No Wi-Fi link: a BLE-only send would be uselessly slow, so nothing is scanned.
    NoLink,
    /// A window, a dial or a transfer is going on.
    Busy,
    /// The previous window started less than [`MIN_SCAN_INTERVAL`] ago.
    TooSoon,
    NoAdapter,
    Failed,
}

#[derive(Debug, Default)]
struct ScanState {
    dedup: Dedup,
    last_start: Option<Instant>,
}

/// Owner of the adapter's discovery: scans and sends exclude each other through `state`.
#[derive(Debug, Default)]
pub struct BleScanner {
    state: tokio::sync::Mutex<ScanState>,
}

pub static SCANNER: Lazy<BleScanner> = Lazy::new(BleScanner::default);

async fn open_adapter() -> anyhow::Result<(bluer::Session, Adapter)> {
    let session = bluer::Session::new().await.context("connect to BlueZ")?;
    let adapter = session
        .default_adapter()
        .await
        .context("no Bluetooth adapter")?;
    if !adapter.is_powered().await? {
        adapter
            .set_powered(true)
            .await
            .context("power the adapter")?;
    }
    Ok((session, adapter))
}

/// Scans until `target` shows up with a PSM; the scan is over when this returns.
async fn find_target(
    adapter: &Adapter,
    target: BleTarget,
    window: Duration,
    ctk: &CancellationToken,
) -> anyhow::Result<ScanResult> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut dedup = Dedup::default();
    let scan = scan_window(adapter, window, &mut dedup, |event| {
        if let Some(r) = target_found(event, target) {
            let _ = tx.send(r);
        }
    });
    tokio::pin!(scan);
    tokio::select! {
        Some(found) = rx.recv() => Ok(found),
        r = &mut scan => {
            r?;
            rx.try_recv().map_err(|_| anyhow::anyhow!(
                "{} not seen within {window:?}: is Quick Share open on it, visible to Everyone?",
                target.id()
            ))
        }
        () = ctk.cancelled() => bail!("cancelled while looking for {}", target.id()),
    }
}

impl BleScanner {
    /// Takes the scan slot, or says why not.
    fn admit(&self, now: Instant) -> Result<MutexGuard<'_, ScanState>, ScanOutcome> {
        let mut state = self.state.try_lock().map_err(|_| ScanOutcome::Busy)?;
        if !interval_elapsed(state.last_start, now) {
            return Err(ScanOutcome::TooSoon);
        }
        state.last_start = Some(now);
        Ok(state)
    }

    /// Runs one window and reports to `sender`. Never fails: whatever keeps it from running
    /// is in the outcome and the debug log.
    pub async fn scan(&self, sender: &broadcast::Sender<EndpointInfo>) -> ScanOutcome {
        if crate::wifi_link().is_none() {
            debug!("BLE scan skipped: no Wi-Fi link, a BLE-only send would be too slow");
            return ScanOutcome::NoLink;
        }
        let mut state = match self.admit(Instant::now()) {
            Ok(state) => state,
            Err(outcome) => {
                debug!("BLE scan skipped: {outcome:?}");
                return outcome;
            }
        };
        let (_session, adapter) = match open_adapter().await {
            Ok(a) => a,
            Err(e) => {
                debug!("BLE scan skipped: {e:#}");
                return ScanOutcome::NoAdapter;
            }
        };
        let result = scan_window(&adapter, SCAN_WINDOW, &mut state.dedup, |event| {
            let _ = sender.send(endpoint_info(&event));
        })
        .await;
        match result {
            Ok(()) => ScanOutcome::Done,
            Err(e) => {
                warn!("BLE scan failed: {e:#}");
                ScanOutcome::Failed
            }
        }
    }

    /// Scans for `target` again (its address and PSM changed since the last window), stops the
    /// scan, dials it and runs the send session, holding the scan slot all along.
    #[allow(clippy::too_many_arguments)]
    pub async fn send(
        &self,
        target: BleTarget,
        local_endpoint_id: [u8; 4],
        id: String,
        name: String,
        payload: OutboundPayload,
        sender: broadcast::Sender<ChannelMessage>,
        ctk: CancellationToken,
    ) -> anyhow::Result<()> {
        let _slot = tokio::select! {
            slot = self.state.lock() => slot,
            () = ctk.cancelled() => bail!("cancelled before the Bluetooth adapter was free"),
        };
        let (_session, adapter) = open_adapter().await?;
        let scan = find_target(&adapter, target, RESCAN_WINDOW, &ctk).await?;
        let stream = dial_l2cap(&adapter, &scan).await?;
        send_over_l2cap(stream, local_endpoint_id, id, name, payload, sender, ctk).await?;
        Ok(())
    }
}

#[cfg(test)]
mod hardware_probe {
    use super::*;

    /// Hand test on a real adapter: open Quick Share on a phone (Everyone), then run
    /// `cargo test -p rqs_lib --no-run`, and execute the printed test binary on the host with
    /// `--ignored --nocapture probe_nearby_phones`.
    #[tokio::test]
    #[ignore = "needs a BlueZ adapter and a phone in Receive mode"]
    async fn probe_nearby_phones() {
        let session = bluer::Session::new().await.expect("BlueZ session");
        let adapter = session.default_adapter().await.expect("adapter");
        adapter.set_powered(true).await.expect("power on");
        let mut dedup = Dedup::default();
        let started = std::time::Instant::now();
        scan_window(&adapter, SCAN_WINDOW, &mut dedup, |event| {
            println!("{:>5.1}s {event:?}", started.elapsed().as_secs_f32());
        })
        .await
        .expect("scan window");
        println!("window over after {:.1}s", started.elapsed().as_secs_f32());
    }

    /// End to end through the public `RQS` API, no app: `scan_ble`, pick the phone from the
    /// `EndpointInfo` bus, send a file as `SendInfo { id: "ble:..." }` with a `WifiLink` that
    /// only prints the offer and fails. Env: `KS_ENDPOINT` (4 character id), `KS_SIZE_KB`
    /// (default 3072). Run the test binary on the host like `probe_nearby_phones`, with
    /// `--ignored --nocapture send_through_rqs_api`; press Accept on the phone.
    #[tokio::test]
    #[ignore = "needs a BlueZ adapter and a phone in Receive mode"]
    async fn send_through_rqs_api() {
        use crate::channel::{ChannelMessage, Message};
        use crate::hdl::TransferState;
        use crate::hdl::ble_send::dial::hardware_probe::ObserverLink;
        use crate::upgrade::WifiLink;
        use crate::{OutboundPayload, RQS, SendInfo, Visibility};

        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "rqs_lib=info".into()),
            )
            .try_init();
        let _guard = crate::upgrade::tests::SERIAL.lock().await;
        let kb: usize = std::env::var("KS_SIZE_KB")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3072);
        let path = std::env::temp_dir().join(format!("kuickshare-rqs-{}.bin", std::process::id()));
        let mut data = vec![0u8; kb * 1024];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut data);
        std::fs::write(&path, data).expect("write the temp file");

        let (offer_tx, mut offer_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut rqs = RQS::new(Visibility::Invisible, None, None, Some("KS probe".into()));
        let link: std::sync::Arc<dyn WifiLink> = std::sync::Arc::new(ObserverLink(offer_tx));
        rqs.set_wifi_link(Some(link));
        let (send_tx, _) = rqs.run().await.expect("run");
        let mut bus = rqs.message_sender.subscribe();

        let (found_tx, mut found_rx) = broadcast::channel(16);
        rqs.scan_ble(found_tx).expect("scan_ble");
        let wanted = std::env::var("KS_ENDPOINT").ok();
        let phone = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let ei = found_rx.recv().await.expect("endpoint bus");
                println!("endpoint {ei:?}");
                let ok = ei.present == Some(true)
                    && ei.ble.is_some_and(|b| {
                        wanted
                            .as_deref()
                            .is_none_or(|w| b.endpoint_id.as_slice() == w.as_bytes())
                    });
                if ok {
                    return ei;
                }
            }
        })
        .await
        .expect("no phone seen in 20 s: is Quick Share open on it, visible to Everyone?");

        send_tx
            .send(SendInfo {
                id: phone.id.clone(),
                name: phone.name.clone().unwrap_or_default(),
                addr: String::new(),
                ob: OutboundPayload::Files(vec![path.to_string_lossy().into_owned()]),
            })
            .await
            .expect("queue the send");
        println!("sending {kb} KB to {}, press Accept on the phone", phone.id);

        let started = std::time::Instant::now();
        let outcome = tokio::time::timeout(Duration::from_secs(300), async {
            loop {
                tokio::select! {
                    offer = offer_rx.recv() => println!("OFFER OBSERVED: {offer:?}"),
                    m = bus.recv() => match m {
                        Ok(ChannelMessage { id, msg: Message::Client(c) }) => {
                            println!("{:>6.1}s {id} {:?}", started.elapsed().as_secs_f32(), c.state);
                            assert_eq!(id, phone.id, "bus id must equal SendInfo.id");
                            if matches!(c.state, Some(TransferState::Finished | TransferState::Disconnected)) {
                                return c.state;
                            }
                        }
                        Ok(_) => {}
                        Err(e) => panic!("bus: {e}"),
                    },
                }
            }
        })
        .await;
        rqs.set_wifi_link(None);
        rqs.stop().await;
        let _ = std::fs::remove_file(&path);
        assert_eq!(outcome.expect("timed out"), Some(TransferState::Finished));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hdl::ble_receiver::service_data::DEVICE_TYPE_LAPTOP;

    fn adv(id: &[u8; 4], name: &str) -> Vec<u8> {
        let mut a = ReceiverAdvertisement::new(*id, 1, [9; 16], name);
        a.psm = Some(161);
        a.to_bytes()
    }

    fn addr(last: u8) -> Address {
        Address::new([0x4a, 0, 0, 0, 0, last])
    }

    fn result(id: &[u8; 4], last: u8, rssi: i16) -> ScanResult {
        scan_result(
            addr(last),
            AddressType::LeRandom,
            Some(rssi),
            &adv(id, "Pixel"),
        )
        .unwrap()
    }

    #[test]
    fn a_phone_becomes_a_result() {
        let r = scan_result(
            addr(1),
            AddressType::LeRandom,
            Some(-60),
            &adv(b"ABCD", "Pixel"),
        );
        assert_eq!(
            r,
            Some(ScanResult {
                endpoint_id: *b"ABCD",
                name: "Pixel".into(),
                device_type: DeviceType::Phone,
                address: addr(1),
                address_type: AddressType::LeRandom,
                psm: Some(161),
                rssi: Some(-60),
            })
        );
    }

    #[test]
    fn other_service_data_is_not_a_result() {
        let fast = [0x4a, 0x03, 1, 2, 3, 0x62, 0xf1];
        assert_eq!(
            scan_result(addr(1), AddressType::LeRandom, None, &fast),
            None
        );
        assert_eq!(scan_result(addr(1), AddressType::LeRandom, None, &[]), None);
        let mut hidden = adv(b"ABCD", "Pixel");
        hidden[17] |= 0x10;
        assert_eq!(
            scan_result(addr(1), AddressType::LeRandom, None, &hidden),
            None
        );
        let laptop = ReceiverAdvertisement::new(*b"ABCD", DEVICE_TYPE_LAPTOP, [0; 16], "L");
        let r = scan_result(addr(1), AddressType::LePublic, None, &laptop.to_bytes()).unwrap();
        assert_eq!(
            (r.device_type, r.psm, r.rssi),
            (DeviceType::Laptop, None, None)
        );
    }

    #[test]
    fn rssi_jitter_is_not_reported_but_a_new_address_is() {
        let mut d = Dedup::default();
        assert!(d.observe(result(b"ABCD", 1, -60)).is_some());
        assert_eq!(d.observe(result(b"ABCD", 1, -70)), None);
        let moved = d.observe(result(b"ABCD", 2, -70)).unwrap();
        assert_eq!(moved.address, addr(2));
    }

    #[test]
    fn a_name_change_is_reported() {
        let mut d = Dedup::default();
        d.observe(result(b"ABCD", 1, -60));
        let renamed = scan_result(
            addr(1),
            AddressType::LeRandom,
            Some(-60),
            &adv(b"ABCD", "New"),
        )
        .unwrap();
        assert!(d.observe(renamed).is_some());
    }

    #[test]
    fn devices_missing_from_a_window_are_gone_once() {
        let mut d = Dedup::default();
        d.begin_window();
        d.observe(result(b"AAAA", 1, -60));
        d.observe(result(b"BBBB", 2, -60));
        assert!(d.end_window().is_empty());

        d.begin_window();
        // Same phone again: nothing to report; the other one stays silent.
        assert_eq!(d.observe(result(b"AAAA", 1, -61)), None);
        assert_eq!(d.end_window(), vec![*b"BBBB"]);

        d.begin_window();
        assert_eq!(d.end_window(), vec![*b"AAAA"]);
        d.begin_window();
        assert!(d.end_window().is_empty());
    }

    #[test]
    fn a_phone_that_comes_back_is_reported_again() {
        let mut d = Dedup::default();
        d.observe(result(b"AAAA", 1, -60));
        d.end_window();
        d.begin_window();
        d.end_window();
        d.begin_window();
        assert!(d.observe(result(b"AAAA", 1, -60)).is_some());
    }

    #[test]
    fn a_cancelled_window_leaves_no_residue() {
        let mut d = Dedup::default();
        d.observe(result(b"AAAA", 1, -60));
        d.end_window();
        d.begin_window();
        d.observe(result(b"BBBB", 2, -60));
        // Cancelled here; the next window sees only AAAA.
        d.begin_window();
        d.observe(result(b"AAAA", 1, -60));
        assert!(d.end_window().is_empty());
    }

    #[test]
    fn found_becomes_a_present_ble_endpoint() {
        let ei = endpoint_info(&ScanEvent::Found(result(b"G483", 1, -60)));
        assert_eq!(ei.id, "ble:G483");
        assert_eq!(ei.fullname, "ble:G483._ble");
        assert_eq!(ei.name.as_deref(), Some("Pixel"));
        assert!(matches!(ei.rtype, Some(DeviceType::Phone)));
        assert_eq!((ei.ip, ei.port), (None, None));
        assert_eq!(ei.present, Some(true));
        assert_eq!(
            ei.ble,
            Some(BleTarget {
                endpoint_id: *b"G483"
            })
        );
    }

    #[test]
    fn gone_becomes_an_absent_ble_endpoint() {
        let ei = endpoint_info(&ScanEvent::Gone(*b"G483"));
        assert_eq!(ei.id, "ble:G483");
        assert_eq!(ei.present, Some(false));
        assert_eq!(ei.name, None);
        assert!(ei.ble.is_some());
        let raw = endpoint_info(&ScanEvent::Gone([0, 1, 2, 3]));
        assert_eq!(raw.id, "ble:00010203");
    }

    #[test]
    fn a_send_waits_only_for_its_phone_with_a_psm() {
        let t = BleTarget {
            endpoint_id: *b"ABCD",
        };
        let hit = result(b"ABCD", 1, -60);
        assert_eq!(
            target_found(ScanEvent::Found(hit.clone()), t),
            Some(hit.clone())
        );
        assert_eq!(
            target_found(ScanEvent::Found(result(b"WXYZ", 1, -60)), t),
            None
        );
        assert_eq!(target_found(ScanEvent::Gone(*b"ABCD"), t), None);
        let no_psm = ScanResult { psm: None, ..hit };
        assert_eq!(target_found(ScanEvent::Found(no_psm), t), None);
    }

    #[test]
    fn windows_start_at_least_the_interval_apart() {
        let t0 = Instant::now();
        assert!(interval_elapsed(None, t0));
        assert!(!interval_elapsed(Some(t0), t0));
        assert!(!interval_elapsed(
            Some(t0),
            t0 + MIN_SCAN_INTERVAL - Duration::from_millis(1)
        ));
        assert!(interval_elapsed(Some(t0), t0 + MIN_SCAN_INTERVAL));
        // A clock that looks backwards never lets a window through by accident.
        assert!(!interval_elapsed(Some(t0 + MIN_SCAN_INTERVAL), t0));
    }

    #[test]
    fn overlapping_and_early_windows_are_refused() {
        let s = BleScanner::default();
        let t0 = Instant::now();
        let slot = s.admit(t0).unwrap();
        assert_eq!(
            s.admit(t0 + MIN_SCAN_INTERVAL).err(),
            Some(ScanOutcome::Busy)
        );
        drop(slot);
        assert_eq!(
            s.admit(t0 + Duration::from_secs(1)).err(),
            Some(ScanOutcome::TooSoon)
        );
        assert!(s.admit(t0 + MIN_SCAN_INTERVAL).is_ok());
    }

    #[tokio::test]
    async fn no_scan_without_a_wifi_link() {
        let _guard = crate::upgrade::tests::SERIAL.lock().await;
        *crate::WIFI_LINK.write().unwrap() = None;
        let s = BleScanner::default();
        let (tx, mut rx) = broadcast::channel(4);
        assert_eq!(s.scan(&tx).await, ScanOutcome::NoLink);
        assert!(rx.try_recv().is_err());
        // The refused call did not use up the rate limit.
        assert!(s.admit(Instant::now()).is_ok());
    }
}
