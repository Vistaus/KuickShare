use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mdns_sd::{AddrType, IfKind, ServiceDaemon, ServiceInfo};
use tokio::sync::watch;
use tokio::time::{Instant, interval_at};
use tokio_util::sync::CancellationToken;

use crate::DEVICE_NAME;
use crate::utils::{DeviceType, gen_mdns_endpoint_info, gen_mdns_name};

const INNER_NAME: &str = "MDnsServer";
const TICK_INTERVAL: Duration = Duration::from_secs(60);
/// Android can miss a service registered before it started discovering (this used to be
/// nudged by a BLE scan of nearby phones); announce again now and then while visible.
const RESEND_INTERVAL: Duration = Duration::from_secs(30);

/// A Wi-Fi Direct group or hotspot has no multicast route, so mDNS sends there fail with
/// "Network is unreachable". Its subnet is 192.168.49.0/24 whether we host it (192.168.49.1)
/// or join the phone's (a client address from the phone's DHCP).
const HOTSPOT_SUBNET: [u8; 3] = [192, 168, 49];

/// A daemon that stays off the hotspot subnet. `IfKind` has no subnet match, so every address
/// of it is disabled up front: the choice outlives interface changes and also covers
/// addresses that appear later.
pub(crate) fn new_daemon() -> Result<ServiceDaemon, anyhow::Error> {
    let daemon = ServiceDaemon::new()?;
    let [a, b, c] = HOTSPOT_SUBNET;
    let hotspot: Vec<IfKind> = (1..=254u8)
        .map(|host| IfKind::Addr(IpAddr::V4(Ipv4Addr::new(a, b, c, host))))
        .collect();
    daemon.disable_interface(hotspot)?;
    Ok(daemon)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Visibility {
    Visible = 0,
    Invisible = 1,
    Temporarily = 2,
}

#[allow(dead_code)]
impl Visibility {
    pub fn from_raw_value(value: u64) -> Self {
        match value {
            0 => Visibility::Visible,
            1 => Visibility::Invisible,
            2 => Visibility::Temporarily,
            _ => unreachable!(),
        }
    }
}

pub struct MDnsServer {
    daemon: ServiceDaemon,
    service_info: ServiceInfo,
    visibility_sender: Arc<Mutex<watch::Sender<Visibility>>>,
    visibility_receiver: watch::Receiver<Visibility>,
}

impl MDnsServer {
    pub fn new(
        endpoint_id: [u8; 4],
        service_port: u16,
        visibility_sender: Arc<Mutex<watch::Sender<Visibility>>>,
        visibility_receiver: watch::Receiver<Visibility>,
    ) -> Result<Self, anyhow::Error> {
        let service_info = Self::build_service(endpoint_id, service_port, DeviceType::Laptop)?;

        Ok(Self {
            daemon: new_daemon()?,
            service_info,
            visibility_sender,
            visibility_receiver,
        })
    }

    pub async fn run(&mut self, ctk: CancellationToken) -> Result<(), anyhow::Error> {
        info!("{INNER_NAME}: service starting");
        let monitor = self.daemon.monitor()?;
        let mut visibility = *self.visibility_receiver.borrow();
        let mut interval = interval_at(Instant::now() + TICK_INTERVAL, TICK_INTERVAL);
        let mut resend = interval_at(Instant::now() + RESEND_INTERVAL, RESEND_INTERVAL);

        loop {
            tokio::select! {
                _ = ctk.cancelled() => {
                    info!("{INNER_NAME}: tracker cancelled, breaking");
                    break;
                }
                r = monitor.recv_async() => {
                    match r {
                        Ok(_) => continue,
                        Err(err) => return Err(err.into()),
                    }
                },
                _ = self.visibility_receiver.changed() => {
                    visibility = *self.visibility_receiver.borrow_and_update();

                    debug!("{INNER_NAME}: visibility changed: {visibility:?}");
                    if visibility == Visibility::Visible {
                        self.daemon.register(self.service_info.clone())?;
                    } else if visibility == Visibility::Invisible {
                        let receiver = self.daemon.unregister(self.service_info.get_fullname())?;
                        let _ = receiver.recv();
                    } else if visibility == Visibility::Temporarily {
                        self.daemon.register(self.service_info.clone())?;
                        interval.reset();
                    }
                }
                _ = resend.tick() => {
                    if visibility == Visibility::Visible || visibility == Visibility::Temporarily {
                        self.daemon.register_resend(self.service_info.get_fullname())?;
                    }
                }
                _ = interval.tick() => {
                    if visibility != Visibility::Temporarily {
                        continue;
                    }

                    let receiver = self.daemon.unregister(self.service_info.get_fullname())?;
                    let _ = receiver.recv();
                    let _ = self.visibility_sender.lock().unwrap().send(Visibility::Invisible);
                }
            }
        }

        // Unregister the mDNS service - we're shutting down
        let receiver = self.daemon.unregister(self.service_info.get_fullname())?;
        if let Ok(event) = receiver.recv() {
            info!("MDnsServer: service unregistered: {:?}", &event);
        }

        Ok(())
    }

    fn build_service(
        endpoint_id: [u8; 4],
        service_port: u16,
        device_type: DeviceType,
    ) -> Result<ServiceInfo, anyhow::Error> {
        // This `name` is going to be random every time RQS service restarts.
        // If that is not desired, derive host_name, etc. via some other means
        let name = gen_mdns_name(endpoint_id);
        let device_name = DEVICE_NAME.read().unwrap().clone();
        info!("Broadcasting with: device_name={device_name}, host_name={name}");
        let endpoint_info = gen_mdns_endpoint_info(device_type as u8, &device_name);

        let properties = [("n", endpoint_info)];
        let si = ServiceInfo::new(
            "_FC9F5ED42C8A._tcp.local.",
            &name,
            &name, // Needs to be ASCII?
            "",
            service_port,
            &properties[..],
        )?
        .enable_addr_auto(AddrType::V4);

        Ok(si)
    }
}
