//! Sending to a phone over BLE L2CAP CoC; we are the central, the phone the receiver.
//!
//! [`send_over_l2cap`] takes an already connected link and runs a whole send session on it:
//! the data connection handshake, UKEY2, the transfer and the Wi-Fi upgrade that the phone
//! should offer, since L2CAP moves only 5-46 KB/s. Nothing here knows BlueZ.
//!
//! What the caller ([`dial::dial_l2cap`]) does:
//! 1. Stop discovery before connecting; a running scan disturbs the connection.
//! 2. Disconnect a stale ACL connection to the phone first, otherwise the connect can hang
//!    or reuse a dead link.
//! 3. Connect to the PSM from the phone's 0xFEF3 advertisement, with the address type of the
//!    scan result.
//! 4. Poll `send_mtu()` of the stream until it succeeds (about 20 ms steps, 20 s at most): on
//!    LE sockets `connect` can return before the channel is usable.
//! 5. Pass the stream here. Dropping it afterwards closes the channel.

pub mod bridge;
#[cfg(all(feature = "experimental", target_os = "linux"))]
pub mod dial;
pub mod l2cap;
#[cfg(test)]
mod tests;

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::broadcast::Sender;
use tokio_util::sync::CancellationToken;

use crate::channel::{self, ChannelMessage, MessageClient, TransferKind};
use crate::errors::AppError;
use crate::hdl::{MigratableStream, OutboundPayload, OutboundRequest, TransferState};
use crate::utils::RemoteDeviceInfo;

use bridge::{BridgeExit, CentralBridge};

const INNER_NAME: &str = "L2capSend";
/// `BLE_L2CAP` of the connection request's `Medium` (not in the upgrade path enum).
const BLE_L2CAP_MEDIUM: i32 = 10;
/// Buffer between the session and the bridge.
const STREAM_BUFFER: usize = 256 * 1024;
/// How long the bridge gets to flush after the session ended.
const BRIDGE_STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// Sends `payload` to the phone at the other end of `link`.
///
/// `id` is the transfer id on the bus. Returns how the channel ended; a refused or
/// unanswered data connection is an error. A session that fails later is reported on the bus
/// as `Disconnected`, like `TcpServer::connect`.
pub async fn send_over_l2cap<L>(
    link: L,
    endpoint_id: [u8; 4],
    id: String,
    name: String,
    payload: OutboundPayload,
    sender: Sender<ChannelMessage>,
    ctk: CancellationToken,
) -> Result<BridgeExit, anyhow::Error>
where
    L: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (session_end, bridge_end) = tokio::io::duplex(STREAM_BUFFER);
    let bridge = tokio::spawn(CentralBridge::new(link, bridge_end).run());

    let mut or = OutboundRequest::new_over(
        endpoint_id,
        MigratableStream::Duplex(session_end),
        id.clone(),
        sender.clone(),
        payload,
        RemoteDeviceInfo {
            device_type: crate::DeviceType::Unknown,
            name,
        },
    );
    if crate::wifi_link().is_none() {
        // No upgrade will follow, so offer none: only the medium we are on.
        or.set_mediums(vec![BLE_L2CAP_MEDIUM]);
    }
    let result = drive(&mut or, &id, &sender, &ctk).await;
    // The session's end of the stream closes, which tells the bridge to finish.
    drop(or);
    let exit = match tokio::time::timeout(BRIDGE_STOP_TIMEOUT, bridge).await {
        Ok(Ok(exit)) => exit,
        Ok(Err(e)) => return Err(anyhow::anyhow!("{INNER_NAME}: bridge task failed: {e}")),
        Err(_) => BridgeExit::LinkClosed,
    };
    match exit {
        BridgeExit::Refused | BridgeExit::HandshakeTimeout => Err(anyhow::anyhow!(
            "{INNER_NAME}: no data connection: {exit:?}"
        )),
        _ => result.map(|()| exit),
    }
}

/// The loop of `TcpServer::connect`.
async fn drive(
    or: &mut OutboundRequest,
    id: &str,
    sender: &Sender<ChannelMessage>,
    ctk: &CancellationToken,
) -> Result<(), anyhow::Error> {
    or.send_connection_request().await?;
    or.send_ukey2_client_init().await?;

    loop {
        tokio::select! {
            _ = ctk.cancelled() => {
                info!("{INNER_NAME}: cancelled");
                return Ok(());
            },
            r = or.handle() => {
                let Err(e) = r else { continue };
                if matches!(e.downcast_ref(), Some(AppError::NotAnError)) {
                    return Ok(());
                }
                if or.state.state == TransferState::Initial {
                    return Ok(());
                }
                if or.state.state != TransferState::Finished
                    && or.state.state != TransferState::Cancelled
                {
                    let _ = sender.send(ChannelMessage {
                        id: id.to_string(),
                        msg: channel::Message::Client(MessageClient {
                            kind: TransferKind::Outbound,
                            state: Some(TransferState::Disconnected),
                            metadata: Default::default(),
                        }),
                    });
                }
                if matches!(or.state.state, TransferState::Finished | TransferState::Cancelled) {
                    // The phone closes the link once it has everything: not a failure.
                    debug!("{INNER_NAME}: session closed after {:?}: {e}", or.state.state);
                } else {
                    error!("{INNER_NAME}: error while handling client: {e} ({:?})", or.state.state);
                }
                return Ok(());
            }
        }
    }
}
