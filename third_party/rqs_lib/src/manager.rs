use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast::Sender;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;

use crate::channel::{self, ChannelMessage, MessageClient, TransferKind};
use crate::errors::AppError;
use crate::hdl::{BleTarget, InboundRequest, OutboundPayload, OutboundRequest, TransferState};
use crate::utils::RemoteDeviceInfo;

const INNER_NAME: &str = "TcpServer";

#[derive(Debug, Clone)]
pub struct SendInfo {
    pub id: String,
    pub name: String,
    pub addr: String,
    pub ob: OutboundPayload,
}

impl SendInfo {
    /// The BLE target when `id` is a `ble:` id (`BleTarget::id`); `addr` is then ignored.
    pub fn ble_target(&self) -> Option<BleTarget> {
        BleTarget::parse(&self.id)
    }
}

/// Runs an inbound session until it ends; an end before `Finished` is reported on the bus.
pub(crate) async fn drive_inbound(mut ir: InboundRequest, sender: Sender<ChannelMessage>) {
    let id = ir.state.id.clone();
    loop {
        match ir.handle().await {
            Ok(_) => {}
            Err(e) => match e.downcast_ref() {
                Some(AppError::NotAnError) => break,
                None => {
                    if ir.state.state == TransferState::Initial {
                        break;
                    }

                    if ir.state.state != TransferState::Finished {
                        let _ = sender.send(ChannelMessage {
                            id,
                            msg: channel::Message::Client(MessageClient {
                                kind: TransferKind::Inbound,
                                state: Some(TransferState::Disconnected),
                                metadata: Default::default(),
                            }),
                        });
                    }
                    error!(
                        "{INNER_NAME}: error while handling client: {e} ({:?})",
                        ir.state.state
                    );
                    break;
                }
            },
        }
    }
}

pub struct TcpServer {
    endpoint_id: [u8; 4],
    tcp_listener: TcpListener,
    sender: Sender<ChannelMessage>,
    connect_receiver: Receiver<SendInfo>,
}

impl TcpServer {
    pub fn new(
        endpoint_id: [u8; 4],
        tcp_listener: TcpListener,
        sender: Sender<ChannelMessage>,
        connect_receiver: Receiver<SendInfo>,
    ) -> Result<Self, anyhow::Error> {
        Ok(Self {
            endpoint_id,
            tcp_listener,
            sender,
            connect_receiver,
        })
    }

    pub async fn run(&mut self, ctk: CancellationToken) -> Result<(), anyhow::Error> {
        info!("{INNER_NAME}: service starting");

        loop {
            let cctk = ctk.clone();

            tokio::select! {
                _ = ctk.cancelled() => {
                    info!("{INNER_NAME}: tracker cancelled, breaking");
                    break;
                }
                Some(i) = self.connect_receiver.recv() => {
                    info!("{INNER_NAME}: connect_receiver: got {:?}", i);
                    if let Err(e) = self.connect(cctk, i).await {
                        error!("{INNER_NAME}: error sending: {}", e.to_string());
                    }
                }
                r = self.tcp_listener.accept() => {
                    match r {
                        Ok((socket, remote_addr)) => {
                            trace!("{INNER_NAME}: new client: {remote_addr}");
                            let ir = InboundRequest::new(socket, remote_addr.to_string(), self.sender.clone());
                            tokio::spawn(drive_inbound(ir, self.sender.clone()));
                        },
                        Err(err) => {
                            error!("{INNER_NAME}: error accepting: {}", err);
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// To be called inside a separate task if we want to handle concurrency
    pub async fn connect(&self, ctk: CancellationToken, si: SendInfo) -> Result<(), anyhow::Error> {
        if let Some(target) = si.ble_target() {
            return self.connect_ble(ctk, si, target).await;
        }
        debug!("{INNER_NAME}: Connecting to: {}", si.addr);
        let socket = TcpStream::connect(si.addr.clone()).await?;

        let mut or = OutboundRequest::new(
            self.endpoint_id,
            socket,
            si.id,
            self.sender.clone(),
            si.ob,
            RemoteDeviceInfo {
                device_type: crate::DeviceType::Unknown,
                name: si.name,
            },
        );

        // Send connection request
        or.send_connection_request().await?;
        // Send UKEY init
        or.send_ukey2_client_init().await?;

        loop {
            tokio::select! {
                _ = ctk.cancelled() => {
                    info!("{INNER_NAME}: tracker cancelled, breaking");
                    break;
                },
                r = or.handle() => {
                    if let Err(e) = r {
                        match e.downcast_ref() {
                            Some(AppError::NotAnError) => break,
                            None => {
                                if or.state.state == TransferState::Initial {
                                    break;
                                }

                                if or.state.state != TransferState::Finished && or.state.state != TransferState::Cancelled {
                                    let _ = self.sender.clone().send(ChannelMessage {
                                        id: si.addr,
                                        msg: channel::Message::Client(MessageClient {
                                            kind: TransferKind::Outbound,
                                            state: Some(TransferState::Disconnected),
                                            metadata: Default::default()
                                        }),
                                    });
                                }
                                error!("{INNER_NAME}: error while handling client: {e} ({:?})", or.state.state);
                                break;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Sends over BLE L2CAP: a fresh scan for the phone, a dial, then the same session and bus
    /// messages as the TCP path, under the transfer id `si.id`.
    async fn connect_ble(
        &self,
        ctk: CancellationToken,
        si: SendInfo,
        target: BleTarget,
    ) -> Result<(), anyhow::Error> {
        if crate::wifi_link().is_none() {
            anyhow::bail!(
                "sending to {} over Bluetooth needs the Wi-Fi upgrade, which is off",
                si.id
            );
        }
        #[cfg(all(feature = "experimental", target_os = "linux"))]
        {
            debug!("{INNER_NAME}: Connecting over BLE to: {}", si.id);
            crate::hdl::ble_scan::SCANNER
                .send(
                    target,
                    self.endpoint_id,
                    si.id,
                    si.name,
                    si.ob,
                    self.sender.clone(),
                    ctk,
                )
                .await
        }
        #[cfg(not(all(feature = "experimental", target_os = "linux")))]
        {
            let _ = (ctk, target);
            anyhow::bail!("sending over Bluetooth is not supported in this build")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upgrade::tests::SERIAL;

    fn ble_info(id: &str) -> SendInfo {
        SendInfo {
            id: id.into(),
            name: "Pixel".into(),
            addr: ":".into(),
            ob: OutboundPayload::Files(vec![]),
        }
    }

    #[test]
    fn only_ble_ids_are_ble_targets() {
        assert_eq!(
            ble_info("ble:G483").ble_target(),
            Some(BleTarget {
                endpoint_id: *b"G483"
            })
        );
        assert_eq!(ble_info("10.0.0.2:4000").ble_target(), None);
    }

    #[tokio::test]
    async fn a_ble_send_without_a_wifi_link_fails_fast() {
        let _guard = SERIAL.lock().await;
        *crate::WIFI_LINK.write().unwrap() = None;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (bus, _rx) = tokio::sync::broadcast::channel(4);
        let (_tx, unused) = tokio::sync::mpsc::channel(1);
        let server = TcpServer::new(*b"TEST", listener, bus, unused).unwrap();
        let err = server
            .connect(CancellationToken::new(), ble_info("ble:G483"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Wi-Fi upgrade"), "{err}");
    }

    #[tokio::test]
    async fn a_tcp_send_does_not_need_the_link() {
        let _guard = SERIAL.lock().await;
        *crate::WIFI_LINK.write().unwrap() = None;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (bus, _rx) = tokio::sync::broadcast::channel(4);
        let (_tx, unused) = tokio::sync::mpsc::channel(1);
        let server = TcpServer::new(*b"TEST", listener, bus, unused).unwrap();
        let mut si = ble_info("127.0.0.1:1");
        si.addr = "127.0.0.1:1".into();
        // Nothing listens there: the failure is a TCP one, not the BLE message.
        let err = server
            .connect(CancellationToken::new(), si)
            .await
            .unwrap_err();
        assert!(!err.to_string().contains("Wi-Fi upgrade"), "{err}");
    }
}
