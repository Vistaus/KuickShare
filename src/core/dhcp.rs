//! Minimal DHCPv4 server for the Wi-Fi Direct group we host.
//!
//! The group has a single client (the phone), so every request gets the same
//! address. Binding to port 67 and to the group interface needs root or
//! `CAP_NET_BIND_SERVICE` + `CAP_NET_RAW`, which the privileged helper has.

use std::{
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
    sync::Arc,
    time::Duration,
};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::{net::UdpSocket, task::JoinSet};

const HEADER_LEN: usize = 236;
const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
const OP_REQUEST: u8 = 1;
const OP_REPLY: u8 = 2;

/// Bound for the neighbour-table child process.
const NEIGH_TIMEOUT: Duration = Duration::from_secs(2);
/// Replies being prepared at once; further packets are dropped (clients retry).
const MAX_IN_FLIGHT: usize = 8;
/// Absolute paths tried for `ip`: the helper runs privileged, so `PATH` must not
/// decide which binary it executes. `PATH` is used only when none exists.
const IP_CANDIDATES: [&str; 4] = ["/usr/sbin/ip", "/usr/bin/ip", "/sbin/ip", "/bin/ip"];

const OPT_PAD: u8 = 0;
const OPT_MASK: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_DNS: u8 = 6;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_LEASE: u8 = 51;
const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_END: u8 = 255;

const DISCOVER: u8 = 1;
const OFFER: u8 = 2;
const REQUEST: u8 = 3;
const ACK: u8 = 5;
const NAK: u8 = 6;

#[derive(Clone, Copy, Debug)]
pub struct DhcpConfig {
    /// Address of the group owner (this machine); also router and DNS.
    pub server_ip: Ipv4Addr,
    /// The only address handed out.
    pub client_ip: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub lease_secs: u32,
}

/// Returns the reply for a client packet, or `None` when it must be ignored.
pub fn build_reply(cfg: &DhcpConfig, req: &[u8]) -> Option<Vec<u8>> {
    if req.len() < HEADER_LEN + 4 || req[0] != OP_REQUEST || req[HEADER_LEN..][..4] != MAGIC_COOKIE
    {
        return None;
    }

    let mut msg_type = None;
    let mut requested_ip = None;
    let mut server_id = None;
    let mut opts = &req[HEADER_LEN + 4..];
    while let [code, rest @ ..] = opts {
        match *code {
            OPT_PAD => opts = rest,
            OPT_END => break,
            code => {
                let (&len, rest) = rest.split_first()?;
                let (value, rest) = rest.split_at_checked(len.into())?;
                match (code, value) {
                    (OPT_MSG_TYPE, [t]) => msg_type = Some(*t),
                    (OPT_REQUESTED_IP, &[a, b, c, d]) => {
                        requested_ip = Some(Ipv4Addr::new(a, b, c, d))
                    }
                    (OPT_SERVER_ID, &[a, b, c, d]) => server_id = Some(Ipv4Addr::new(a, b, c, d)),
                    _ => {}
                }
                opts = rest;
            }
        }
    }

    let reply_type = match msg_type? {
        DISCOVER => OFFER,
        REQUEST => {
            // The client picked another server.
            if server_id.is_some_and(|s| s != cfg.server_ip) {
                return None;
            }
            // `ciaddr` is set when renewing; `requested_ip` otherwise.
            let ciaddr = Ipv4Addr::new(req[12], req[13], req[14], req[15]);
            let wanted = requested_ip.unwrap_or(ciaddr);
            if wanted == cfg.client_ip { ACK } else { NAK }
        }
        _ => return None,
    };

    let mut out = vec![0u8; HEADER_LEN];
    out[0] = OP_REPLY;
    out[1..4].copy_from_slice(&req[1..4]); // htype, hlen, hops
    out[4..8].copy_from_slice(&req[4..8]); // xid
    out[10..12].copy_from_slice(&req[10..12]); // flags
    if reply_type != NAK {
        out[16..20].copy_from_slice(&cfg.client_ip.octets()); // yiaddr
        out[20..24].copy_from_slice(&cfg.server_ip.octets()); // siaddr
    }
    out[28..44].copy_from_slice(&req[28..44]); // chaddr
    out.extend_from_slice(&MAGIC_COOKIE);
    out.extend_from_slice(&[OPT_MSG_TYPE, 1, reply_type]);
    out.extend_from_slice(&[OPT_SERVER_ID, 4]);
    out.extend_from_slice(&cfg.server_ip.octets());
    if reply_type != NAK {
        out.extend_from_slice(&[OPT_LEASE, 4]);
        out.extend_from_slice(&cfg.lease_secs.to_be_bytes());
        out.extend_from_slice(&[OPT_MASK, 4]);
        out.extend_from_slice(&cfg.netmask.octets());
        out.extend_from_slice(&[OPT_ROUTER, 4]);
        out.extend_from_slice(&cfg.server_ip.octets());
        // The host runs no DNS server. Kept because some Android versions treat
        // an empty DNS list as a failed configuration (unverified).
        out.extend_from_slice(&[OPT_DNS, 4]);
        out.extend_from_slice(&cfg.server_ip.octets());
    }
    out.push(OPT_END);
    Some(out)
}

/// Opens port 67 on `iface` only, so no other network sees our replies.
pub fn bind(iface: &str) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_broadcast(true)?;
    socket.bind_device(Some(iface.as_bytes()))?;
    socket.bind(&SocketAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 67)).into())?;
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket.into())
}

/// Where a reply goes: unicast to the client when it did not ask for a
/// broadcast (RFC 2131 4.1), because access points may drop broadcast frames
/// for clients in power save.
fn destination(reply: &[u8]) -> (SocketAddrV4, Option<[u8; 6]>) {
    let broadcast_flag = reply[10] & 0x80 != 0;
    let yiaddr = Ipv4Addr::new(reply[16], reply[17], reply[18], reply[19]);
    if broadcast_flag || yiaddr.is_unspecified() || reply[2] != 6 {
        return (SocketAddrV4::new(Ipv4Addr::BROADCAST, 68), None);
    }
    let Ok(mac) = reply[28..34].try_into() else {
        return (SocketAddrV4::new(Ipv4Addr::BROADCAST, 68), None);
    };
    (SocketAddrV4::new(yiaddr, 68), Some(mac))
}

fn first_existing<'a>(candidates: &[&'a str]) -> Option<&'a str> {
    candidates.iter().copied().find(|p| Path::new(p).exists())
}

/// The client has no address yet, so the kernel cannot resolve its MAC:
/// give it the entry.
async fn set_neighbour(iface: &str, ip: Ipv4Addr, mac: [u8; 6]) -> io::Result<()> {
    let mac = mac.map(|b| format!("{b:02x}")).join(":");
    let mut cmd = tokio::process::Command::new(first_existing(&IP_CANDIDATES).unwrap_or("ip"));
    cmd.kill_on_drop(true).args([
        "neigh",
        "replace",
        &ip.to_string(),
        "lladdr",
        &mac,
        "dev",
        iface,
        "nud",
        "permanent",
    ]);
    let status = tokio::time::timeout(NEIGH_TIMEOUT, cmd.status())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "ip neigh timed out"))??;
    status
        .success()
        .then_some(())
        .ok_or_else(|| io::Error::other(format!("ip neigh failed: {status}")))
}

/// The `chaddr` field (16 bytes) of a client packet.
fn chaddr(req: &[u8]) -> Option<[u8; 16]> {
    req.get(28..44)?.try_into().ok()
}

/// Whether `req` may be served: any client until one completed a lease, then only that one.
fn admits(served: Option<[u8; 16]>, req: &[u8]) -> bool {
    served.is_none_or(|s| chaddr(req) == Some(s))
}

fn is_ack(reply: &[u8]) -> bool {
    reply.get(HEADER_LEN + 4..HEADER_LEN + 7) == Some(&[OPT_MSG_TYPE, 1, ACK])
}

/// The client is locked in only by a completed REQUEST -> ACK, so a phone that
/// randomises its MAC between attempts is not shut out by an earlier DISCOVER.
fn lock_after(served: Option<[u8; 16]>, req: &[u8], reply: &[u8]) -> Option<[u8; 16]> {
    if is_ack(reply) { chaddr(req) } else { served }
}

async fn send_reply(socket: &UdpSocket, iface: &str, reply: Vec<u8>) {
    let (mut to, mac) = destination(&reply);
    if let Some(mac) = mac
        && let Err(e) = set_neighbour(iface, *to.ip(), mac).await
    {
        tracing::warn!("{e}; falling back to broadcast");
        to = SocketAddrV4::new(Ipv4Addr::BROADCAST, 68);
    }
    if let Err(e) = socket.send_to(&reply, to).await {
        tracing::warn!("DHCP reply failed: {e}");
    }
}

/// Answers clients on `iface` until the future is dropped (which also stops
/// the pending replies). Once a client got an ACK, only it is served.
pub async fn serve(socket: UdpSocket, iface: &str, cfg: DhcpConfig) -> io::Result<()> {
    let socket = Arc::new(socket);
    let mut buf = [0u8; 1500];
    let mut served = None;
    let mut replies = JoinSet::new();
    loop {
        let (n, _) = socket.recv_from(&mut buf).await?;
        while replies.try_join_next().is_some() {}
        let req = &buf[..n];
        if !admits(served, req) || replies.len() >= MAX_IN_FLIGHT {
            continue;
        }
        let Some(reply) = build_reply(&cfg, req) else {
            continue;
        };
        served = lock_after(served, req, &reply);
        let (socket, iface) = (socket.clone(), iface.to_owned());
        replies.spawn(async move { send_reply(&socket, &iface, reply).await });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: DhcpConfig = DhcpConfig {
        server_ip: Ipv4Addr::new(192, 168, 49, 1),
        client_ip: Ipv4Addr::new(192, 168, 49, 2),
        netmask: Ipv4Addr::new(255, 255, 255, 0),
        lease_secs: 3600,
    };

    fn request(msg_type: u8, extra: &[u8]) -> Vec<u8> {
        let mut p = vec![0u8; HEADER_LEN];
        p[0] = OP_REQUEST;
        p[1] = 1; // ethernet
        p[2] = 6;
        p[4..8].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        p[28..34].copy_from_slice(&[2, 0, 0, 0, 0, 1]);
        p.extend_from_slice(&MAGIC_COOKIE);
        p.extend_from_slice(&[OPT_MSG_TYPE, 1, msg_type]);
        p.extend_from_slice(extra);
        p.push(OPT_END);
        p
    }

    /// Option value from a reply, searching after the cookie.
    fn option(reply: &[u8], wanted: u8) -> Option<Vec<u8>> {
        let mut opts = &reply[HEADER_LEN + 4..];
        while let [code, rest @ ..] = opts {
            match *code {
                OPT_PAD => opts = rest,
                OPT_END => return None,
                c => {
                    let (&len, rest) = rest.split_first()?;
                    let (value, rest) = rest.split_at(len.into());
                    if c == wanted {
                        return Some(value.to_vec());
                    }
                    opts = rest;
                }
            }
        }
        None
    }

    #[test]
    fn discover_gets_offer() {
        let r = build_reply(&CFG, &request(DISCOVER, &[])).unwrap();
        assert_eq!(r[0], OP_REPLY);
        assert_eq!(r[4..8], [0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(r[16..20], CFG.client_ip.octets());
        assert_eq!(option(&r, OPT_MSG_TYPE).unwrap(), [OFFER]);
        assert_eq!(option(&r, OPT_SERVER_ID).unwrap(), CFG.server_ip.octets());
        assert_eq!(option(&r, OPT_MASK).unwrap(), CFG.netmask.octets());
        assert_eq!(option(&r, OPT_ROUTER).unwrap(), CFG.server_ip.octets());
    }

    #[test]
    fn request_for_our_address_gets_ack() {
        let mut extra = vec![OPT_REQUESTED_IP, 4];
        extra.extend_from_slice(&CFG.client_ip.octets());
        let r = build_reply(&CFG, &request(REQUEST, &extra)).unwrap();
        assert_eq!(option(&r, OPT_MSG_TYPE).unwrap(), [ACK]);
        assert_eq!(option(&r, OPT_LEASE).unwrap(), 3600u32.to_be_bytes());
    }

    #[test]
    fn request_for_other_address_gets_nak() {
        let extra = [OPT_REQUESTED_IP, 4, 10, 0, 0, 9];
        let r = build_reply(&CFG, &request(REQUEST, &extra)).unwrap();
        assert_eq!(option(&r, OPT_MSG_TYPE).unwrap(), [NAK]);
        assert_eq!(r[16..20], [0; 4]);
    }

    #[test]
    fn request_to_another_server_is_ignored() {
        let extra = [OPT_SERVER_ID, 4, 10, 0, 0, 1];
        assert!(build_reply(&CFG, &request(REQUEST, &extra)).is_none());
    }

    #[test]
    fn replies_are_unicast_unless_the_client_asks_for_broadcast() {
        let offer = build_reply(&CFG, &request(DISCOVER, &[])).unwrap();
        let (to, mac) = destination(&offer);
        assert_eq!(to, SocketAddrV4::new(CFG.client_ip, 68));
        assert_eq!(mac, Some([2, 0, 0, 0, 0, 1]));

        let mut asks_broadcast = request(DISCOVER, &[]);
        asks_broadcast[10] = 0x80;
        let offer = build_reply(&CFG, &asks_broadcast).unwrap();
        assert_eq!(
            destination(&offer),
            (SocketAddrV4::new(Ipv4Addr::BROADCAST, 68), None)
        );

        let nak =
            build_reply(&CFG, &request(REQUEST, &[OPT_REQUESTED_IP, 4, 10, 0, 0, 9])).unwrap();
        assert_eq!(destination(&nak).1, None);
    }

    #[test]
    fn only_the_client_that_got_an_ack_is_served() {
        let first = request(DISCOVER, &[]);
        let mut other = first.clone();
        other[33] = 9;
        let offer = build_reply(&CFG, &first).unwrap();
        // An OFFER does not lock: another chaddr may still get served.
        let served = lock_after(None, &first, &offer);
        assert_eq!(served, None);
        assert!(admits(served, &other));

        let mut extra = vec![OPT_REQUESTED_IP, 4];
        extra.extend_from_slice(&CFG.client_ip.octets());
        let req = request(REQUEST, &extra);
        let ack = build_reply(&CFG, &req).unwrap();
        let served = lock_after(None, &req, &ack);
        assert_eq!(served, chaddr(&first));
        assert!(admits(served, &first));
        assert!(!admits(served, &other));
        assert!(!admits(served, &[0; 10]));

        // A NAK keeps the previous state.
        let nak =
            build_reply(&CFG, &request(REQUEST, &[OPT_REQUESTED_IP, 4, 10, 0, 0, 9])).unwrap();
        assert_eq!(lock_after(None, &req, &nak), None);
    }

    #[test]
    fn ip_binary_is_taken_from_the_first_existing_absolute_path() {
        assert_eq!(first_existing(&["/nonexistent/ip", "/", "/tmp"]), Some("/"));
        assert_eq!(first_existing(&["/nonexistent/ip"]), None);
    }

    #[test]
    fn garbage_is_ignored() {
        assert!(build_reply(&CFG, &[]).is_none());
        assert!(build_reply(&CFG, &[0; 300]).is_none());
        let mut truncated = request(DISCOVER, &[]);
        truncated.truncate(HEADER_LEN + 6);
        assert!(build_reply(&CFG, &truncated).is_none());
    }
}
