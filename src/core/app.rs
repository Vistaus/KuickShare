//! Owns the Quick Share service and the user settings; rebuilds the service when needed.

use std::{
    collections::{HashMap, HashSet},
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use rqs_lib::{
    BleTarget, EndpointInfo, OutboundPayload, RQS, SendInfo, TextPayloadType, Visibility, WifiLink,
    channel::{ChannelMessage, Message, TransferAction},
};
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
};
use zbus::Connection;

use crate::{
    allowed::{self, Allowed, AllowedDevice},
    dbus::{self, TransferEvent},
    history::{self, History},
    link::NmLink,
    settings::{self, Settings},
    trust::{self, Trust, TrustEvent},
};

/// Nearby devices found by discovery, keyed by endpoint id (`ip:port`, or `ble:<id>` over Bluetooth).
pub type Endpoints = Arc<StdMutex<HashMap<String, EndpointInfo>>>;
/// IPs of nearby computers that run KuickShare and are paired with this one.
pub type TrustedIps = Arc<StdMutex<HashSet<String>>>;
/// Transfers that have not reached a final state yet.
pub type ActiveTransfers = Arc<StdMutex<HashMap<String, TransferEvent>>>;
pub type SharedHistory = Arc<StdMutex<History>>;

struct Engine {
    rqs: RQS,
    send_tx: mpsc::Sender<SendInfo>,
    tasks: Vec<JoinHandle<()>>,
    discovery_tx: broadcast::Sender<EndpointInfo>,
    discovery_on: bool,
}

impl Engine {
    async fn stop(mut self) {
        for task in &self.tasks {
            task.abort();
        }
        self.rqs.stop().await;
    }
}

pub struct Core {
    settings: Settings,
    settings_path: Option<PathBuf>,
    engine: Option<Engine>,
    endpoints: Endpoints,
    active: ActiveTransfers,
    history: SharedHistory,
    trust: Arc<Trust>,
    trust_events: Option<mpsc::UnboundedReceiver<TrustEvent>>,
    trusted_ips: TrustedIps,
    allowed: Allowed,
    /// Whether the allowed list is used; shared with the task that accepts transfers.
    trusted_enabled: Arc<AtomicBool>,
    conn: Option<Connection>,
    /// Temporary hiding (not persisted), e.g. while Do Not Disturb is on.
    suspended: bool,
}

impl Core {
    pub fn new(
        settings: Settings,
        settings_path: Option<PathBuf>,
        history: History,
        trust: Arc<Trust>,
        trust_events: mpsc::UnboundedReceiver<TrustEvent>,
    ) -> Self {
        let mut history = history;
        history.set_policy(settings.history_enabled, settings.history_keep_days);
        Self {
            allowed: Allowed::new(StdMutex::new(settings.allowed_devices.clone())),
            trusted_enabled: Arc::new(AtomicBool::new(settings.trusted_devices)),
            settings,
            settings_path,
            engine: None,
            endpoints: Endpoints::default(),
            active: ActiveTransfers::default(),
            history: Arc::new(StdMutex::new(history)),
            trust,
            trust_events: Some(trust_events),
            trusted_ips: TrustedIps::default(),
            conn: None,
            suspended: false,
        }
    }

    pub async fn start(&mut self, conn: Connection) -> Result<()> {
        if let Some(events) = self.trust_events.take() {
            tokio::spawn(dbus::forward_trust_events(conn.clone(), events));
        }
        let any = SocketAddr::from(([0, 0, 0, 0], trust::DEFAULT_PORT));
        if let Err(e) = self.trust.listen(any).await {
            // Trusted devices are optional: sharing works without them.
            tracing::warn!("trusted devices unavailable: {e:#}");
        }
        self.conn = Some(conn);
        if self.settings.wifi_upgrade {
            remove_stale_wifi_profiles().await;
        }
        self.start_engine(false).await
    }

    pub async fn shutdown(&mut self) {
        if let Some(engine) = self.engine.take() {
            engine.stop().await;
        }
        if self.settings.wifi_upgrade {
            remove_stale_wifi_profiles().await;
        }
    }

    pub fn trust(&self) -> Arc<Trust> {
        self.trust.clone()
    }

    /// Starts pairing with a nearby computer; the outcome arrives as trust events.
    pub fn pair_with(&self, endpoint_id: &str) -> Result<()> {
        let addr = self.control_addr(endpoint_id)?;
        let trust = self.trust.clone();
        tokio::spawn(async move {
            if let Err(e) = trust.pair(addr).await {
                tracing::info!("pairing failed: {e:#}");
            }
        });
        Ok(())
    }

    fn control_addr(&self, endpoint_id: &str) -> Result<SocketAddr> {
        if BleTarget::parse(endpoint_id).is_some() {
            bail!("Bluetooth devices cannot be paired");
        }
        let ip = self
            .endpoints
            .lock()
            .unwrap()
            .get(endpoint_id)
            .and_then(|e| e.ip.clone())
            .with_context(|| format!("unknown device {endpoint_id}"))?;
        Ok(SocketAddr::new(ip.parse()?, trust::DEFAULT_PORT))
    }

    /// Remembers the sender of a transfer in progress: from now on its transfers are accepted
    /// without asking. Needs the device name and its MAC address (same network only).
    pub fn allow_device(&mut self, transfer_id: &str) -> Result<()> {
        let name = self
            .active
            .lock()
            .unwrap()
            .get(transfer_id)
            .map(|t| t.device().to_owned())
            .filter(|n| !n.is_empty())
            .context("unknown transfer or device without a name")?;
        let mac = transfer_id
            .rsplit_once(':')
            .and_then(|(ip, _)| allowed::mac_for_ip(ip))
            .context("cannot read the MAC address of this device (only devices on the same network can be trusted)")?;
        let device = AllowedDevice { name, mac };
        let mut list = self.allowed.lock().unwrap();
        if !list.contains(&device) {
            list.push(device);
        }
        self.settings.allowed_devices = list.clone();
        drop(list);
        self.save();
        Ok(())
    }

    pub fn forget_allowed(&mut self, name: &str, mac: &str) {
        let mut list = self.allowed.lock().unwrap();
        list.retain(|d| !(d.name == name && d.mac == mac));
        self.settings.allowed_devices = list.clone();
        drop(list);
        self.save();
    }

    pub fn allowed_json(&self) -> String {
        serde_json::to_string(&*self.allowed.lock().unwrap()).unwrap_or_else(|_| "[]".to_owned())
    }

    pub fn history_json(&self) -> String {
        self.history.lock().unwrap().json()
    }

    pub fn clear_history(&self) {
        self.history.lock().unwrap().clear();
    }

    pub fn history_size(&self) -> u64 {
        self.history.lock().unwrap().file_size()
    }

    pub fn remove_history(&self, uid: u64) {
        self.history.lock().unwrap().remove(uid);
    }

    pub fn set_history_enabled(&mut self, enabled: bool) {
        self.settings.history_enabled = enabled;
        self.apply_history_policy();
    }

    pub fn set_history_keep_days(&mut self, days: u32) {
        self.settings.history_keep_days = days;
        self.apply_history_policy();
    }

    /// Puts every setting back to its default. The list of trusted devices is kept: it is data the user
    /// built up, not a preference. Fails, leaving things as they are, while a transfer is running.
    pub async fn reset_settings(&mut self) -> Result<()> {
        self.ensure_idle()?;
        let previous = self.settings.clone();
        self.settings = Settings {
            allowed_devices: previous.allowed_devices.clone(),
            ..Settings::default()
        };
        self.trusted_enabled
            .store(self.settings.trusted_devices, Ordering::Relaxed);
        self.history.lock().unwrap().set_policy(
            self.settings.history_enabled,
            self.settings.history_keep_days,
        );
        // The name or the port may differ now: restart the service with the defaults.
        if let Err(e) = self.restart_or_revert(previous.clone()).await {
            // restart_or_revert put the settings back; what depends on them goes back too.
            self.trusted_enabled
                .store(previous.trusted_devices, Ordering::Relaxed);
            self.history
                .lock()
                .unwrap()
                .set_policy(previous.history_enabled, previous.history_keep_days);
            return Err(e);
        }
        self.trust.set_name(self.device_name());
        Ok(())
    }

    pub fn set_trusted_devices(&mut self, enabled: bool) {
        self.settings.trusted_devices = enabled;
        self.trusted_enabled.store(enabled, Ordering::Relaxed);
        self.save();
    }

    pub fn set_wifi_upgrade(&mut self, enabled: bool) {
        self.settings.wifi_upgrade = enabled;
        if let Some(engine) = &self.engine {
            engine.rqs.set_wifi_link(wifi_link(enabled));
        }
        self.save();
    }

    pub fn set_tray_icon(&mut self, show: bool) {
        self.settings.tray_icon = show;
        self.save();
    }

    pub fn set_dolphin_integration(&mut self, enabled: bool) {
        self.settings.dolphin_integration = enabled;
        self.save();
    }

    fn apply_history_policy(&self) {
        self.history.lock().unwrap().set_policy(
            self.settings.history_enabled,
            self.settings.history_keep_days,
        );
        self.save();
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn device_name(&self) -> String {
        self.settings
            .device_name
            .clone()
            .unwrap_or_else(settings::default_device_name)
    }

    pub async fn set_device_name(&mut self, name: &str) -> Result<()> {
        let name = Some(name.trim().to_owned()).filter(|n| !n.is_empty());
        if name == self.settings.device_name {
            return Ok(());
        }
        self.ensure_idle()?;
        let previous = self.settings.clone();
        self.settings.device_name = name;
        self.restart_or_revert(previous).await?;
        self.trust.set_name(self.device_name());
        Ok(())
    }

    pub async fn set_static_port(&mut self, port: u32) -> Result<()> {
        if port != 0 && !(1024..=65535).contains(&port) {
            bail!("Choose a port between 1024 and 65535");
        }
        let port = Some(port).filter(|&p| p != 0);
        if port == self.settings.static_port {
            return Ok(());
        }
        self.ensure_idle()?;
        let previous = self.settings.clone();
        self.settings.static_port = port;
        self.restart_or_revert(previous).await.map_err(|_| {
            anyhow!(
                "Port {} can't be used, another program is probably using it",
                port.unwrap_or_default()
            )
        })
    }

    pub fn set_download_dir(&mut self, dir: &str) -> Result<()> {
        let dir = Some(PathBuf::from(dir.trim())).filter(|d| !d.as_os_str().is_empty());
        if let Some(dir) = &dir {
            fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        }
        if let Some(engine) = &self.engine {
            engine.rqs.set_download_path(dir.clone());
        }
        self.settings.download_dir = dir;
        self.save();
        Ok(())
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.settings.visible = visible;
        self.apply_visibility();
        self.save();
    }

    pub fn suspended(&self) -> bool {
        self.suspended
    }

    pub fn set_suspended(&mut self, suspended: bool) {
        self.suspended = suspended;
        self.apply_visibility();
    }

    pub fn set_hide_when_dnd(&mut self, hide: bool) {
        self.settings.hide_when_dnd = hide;
        self.save();
    }

    fn apply_visibility(&mut self) {
        let visible = self.settings.visible && !self.suspended;
        if let Some(engine) = &mut self.engine {
            engine.rqs.change_visibility(visibility(visible));
        }
    }

    /// Consent and cancel actions travel on the library's own event channel.
    pub fn send_lib_action(&self, id: &str, action: TransferAction) -> Result<()> {
        let engine = self.engine.as_ref().context("service is not running")?;
        engine
            .rqs
            .message_sender
            .send(ChannelMessage {
                id: id.to_owned(),
                msg: Message::Lib { action },
            })
            .map(|_| ())
            .map_err(|e| anyhow!(e.to_string()))
    }

    pub fn start_discovery(&mut self) -> Result<()> {
        let engine = self.engine.as_mut().context("service is not running")?;
        if !engine.discovery_on {
            engine.rqs.discovery(engine.discovery_tx.clone())?;
            engine.discovery_on = true;
        }
        Ok(())
    }

    /// Runs one more Bluetooth scan window; the devices arrive like the ones of `start_discovery`.
    pub fn scan_bluetooth(&self) -> Result<()> {
        let engine = self.engine.as_ref().context("service is not running")?;
        if !engine.discovery_on {
            bail!("discovery is not running");
        }
        engine.rqs.scan_ble(engine.discovery_tx.clone())
    }

    pub fn stop_discovery(&mut self) {
        if let Some(engine) = &mut self.engine
            && engine.discovery_on
        {
            engine.rqs.stop_discovery();
            engine.discovery_on = false;
        }
        self.endpoints.lock().unwrap().clear();
    }

    pub async fn send_files(&self, endpoint_id: &str, paths: Vec<String>) -> Result<()> {
        if paths.is_empty() {
            bail!("no files to send");
        }
        for path in &paths {
            fs::metadata(path).with_context(|| format!("cannot read {path}"))?;
        }
        self.send(endpoint_id, OutboundPayload::Files(paths)).await
    }

    pub async fn send_text(&self, endpoint_id: &str, text: String, url: bool) -> Result<()> {
        if text.trim().is_empty() {
            bail!("nothing to send");
        }
        let text_type = if url {
            TextPayloadType::Url
        } else {
            TextPayloadType::Text
        };
        self.send(endpoint_id, OutboundPayload::Text { text, text_type })
            .await
    }

    async fn send(&self, endpoint_id: &str, ob: OutboundPayload) -> Result<()> {
        let engine = self.engine.as_ref().context("service is not running")?;
        let endpoint = self
            .endpoints
            .lock()
            .unwrap()
            .get(endpoint_id)
            .cloned()
            .with_context(|| format!("unknown device {endpoint_id}"))?;
        let ble = endpoint.ble.is_some();
        if ble && !self.settings.wifi_upgrade {
            bail!("Sending over Bluetooth needs the Wi-Fi upgrade, which is off (see Settings)");
        }
        // A paired KuickShare computer accepts the transfer by itself if it is told in advance.
        let trusted = endpoint
            .ip
            .as_ref()
            .is_some_and(|ip| self.trusted_ips.lock().unwrap().contains(ip));
        if trusted && let Ok(addr) = self.control_addr(endpoint_id) {
            let announced =
                tokio::time::timeout(Duration::from_secs(2), self.trust.announce(addr)).await;
            if !matches!(announced, Ok(Ok(()))) {
                tracing::info!("could not announce the transfer to the paired computer");
            }
        }
        let name = endpoint.name.clone().unwrap_or_default();
        let addr = if ble {
            String::new()
        } else {
            format!(
                "{}:{}",
                endpoint.ip.unwrap_or_default(),
                endpoint.port.unwrap_or_default()
            )
        };
        // The library only logs a failed scan or dial: without any event in time, tell the user.
        let watch = if let (true, Some(conn)) = (ble, &self.conn) {
            Some((conn.clone(), engine.rqs.message_sender.subscribe()))
        } else {
            None
        };
        let send_info = SendInfo {
            id: endpoint.id.clone(),
            name: name.clone(),
            addr,
            ob,
        };
        engine
            .send_tx
            .send(send_info)
            .await
            .context("service stopped while sending")?;
        if let Some((conn, events)) = watch {
            tokio::spawn(report_silent_send(conn, events, endpoint.id, name));
        }
        Ok(())
    }

    fn ensure_idle(&self) -> Result<()> {
        let mut active = self.active.lock().unwrap();
        drop_stale(&mut active, history::now());
        if active.is_empty() {
            Ok(())
        } else {
            bail!("Wait for the current transfer to finish before changing this setting");
        }
    }

    /// Applies the new settings by restarting the service; if that fails, the previous settings
    /// are restored (and the service restarted) so sharing keeps working.
    async fn restart_or_revert(&mut self, previous: Settings) -> Result<()> {
        match self.restart().await {
            Ok(()) => {
                self.save();
                Ok(())
            }
            Err(e) => {
                tracing::warn!("restart failed, reverting settings: {e:#}");
                self.settings = previous;
                if let Err(e) = self.restart().await {
                    tracing::error!("cannot restart with the previous settings: {e:#}");
                }
                Err(e)
            }
        }
    }

    /// Rebuilds the service, keeping discovery on if it was on.
    async fn restart(&mut self) -> Result<()> {
        let discovery = self.engine.as_ref().is_some_and(|e| e.discovery_on);
        if let Some(engine) = self.engine.take() {
            engine.stop().await;
        }
        self.endpoints.lock().unwrap().clear();
        self.start_engine(discovery).await
    }

    async fn start_engine(&mut self, discovery: bool) -> Result<()> {
        let conn = self.conn.clone().context("not connected to the bus")?;
        let mut rqs = RQS::new(
            visibility(self.settings.visible && !self.suspended),
            self.settings.static_port,
            self.settings.download_dir.clone(),
            Some(self.device_name()),
        );
        rqs.set_wifi_link(wifi_link(self.settings.wifi_upgrade));
        let (send_tx, _) = rqs
            .run()
            .await
            .context("failed to start the Quick Share service")?;

        let (discovery_tx, _) = broadcast::channel(32);
        let tasks = dbus::spawn_forwarders(
            dbus::Forwarding {
                connection: conn,
                endpoints: self.endpoints.clone(),
                active: self.active.clone(),
                history: self.history.clone(),
                trust: self.trust.clone(),
                trusted_ips: self.trusted_ips.clone(),
                allowed: self.allowed.clone(),
                trusted_enabled: self.trusted_enabled.clone(),
                lib_sender: rqs.message_sender.clone(),
            },
            rqs.message_sender.subscribe(),
            discovery_tx.subscribe(),
        );
        self.engine = Some(Engine {
            rqs,
            send_tx,
            tasks,
            discovery_tx,
            discovery_on: false,
        });
        if discovery {
            self.start_discovery()?;
        }
        Ok(())
    }

    fn save(&self) {
        if let Some(path) = &self.settings_path
            && let Err(e) = self.settings.save(path)
        {
            tracing::warn!("cannot save settings: {e:#}");
        }
    }
}

/// A Bluetooth send scans for at most 12 s and then dials.
const BLE_START_TIMEOUT: Duration = Duration::from_secs(25);

/// Waits for the first event of the transfer `id`; if none comes in time, the scan or the dial
/// failed (the library only logs that) and the transfer is reported as failed.
async fn report_silent_send(
    conn: Connection,
    mut events: broadcast::Receiver<ChannelMessage>,
    id: String,
    device: String,
) {
    let started = async {
        loop {
            match events.recv().await {
                Ok(message) if message.id == id => return true,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                // The service stopped: nothing to report.
                Err(broadcast::error::RecvError::Closed) => return true,
            }
        }
    };
    if tokio::time::timeout(BLE_START_TIMEOUT, started)
        .await
        .is_ok()
    {
        return;
    }
    tracing::info!("no answer from {id}: reporting the Bluetooth send as failed");
    dbus::emit_failed_transfer(&conn, &id, &device).await;
}

/// A transfer that has not been heard of for this long is taken to be over (its final event was
/// lost), so it does not block the settings that restart the service for ever.
const STALE_AFTER_SECS: i64 = 600;

fn drop_stale(active: &mut HashMap<String, TransferEvent>, now: i64) {
    active.retain(|_, transfer| now - transfer.last_event < STALE_AFTER_SECS);
}

/// Removes `kuickshare-*` Wi-Fi profiles left by a crash or by the shutdown, only when the Wi-Fi
/// upgrade is on (it is the only user of NetworkManager). Quiet when NetworkManager is absent;
/// bounded so it cannot hang the daemon.
async fn remove_stale_wifi_profiles() {
    let link = NmLink::new();
    let sweep = link.cleanup_stale();
    if tokio::time::timeout(Duration::from_secs(5), sweep)
        .await
        .is_err()
    {
        tracing::debug!("Wi-Fi profile cleanup timed out");
    }
}

fn wifi_link(enabled: bool) -> Option<Arc<dyn WifiLink>> {
    enabled.then(|| Arc::new(NmLink::new()) as Arc<dyn WifiLink>)
}

fn visibility(visible: bool) -> Visibility {
    if visible {
        Visibility::Visible
    } else {
        Visibility::Invisible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer(last_event: i64) -> TransferEvent {
        let mut transfer = TransferEvent::default();
        transfer.last_event = last_event;
        transfer
    }

    #[test]
    fn transfers_not_heard_of_for_a_long_time_stop_counting_as_active() {
        let now = 10_000;
        let mut active = HashMap::from([
            ("recent".to_owned(), transfer(now - 5)),
            (
                "quiet but alive".to_owned(),
                transfer(now - STALE_AFTER_SECS + 1),
            ),
            ("lost".to_owned(), transfer(now - STALE_AFTER_SECS)),
        ]);
        drop_stale(&mut active, now);
        let mut left: Vec<_> = active.keys().cloned().collect();
        left.sort();
        assert_eq!(left, ["quiet but alive", "recent"]);
    }
}
