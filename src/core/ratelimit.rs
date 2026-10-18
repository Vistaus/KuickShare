//! A per-address limit on how often an address may connect to the trusted-devices port.

use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    time::{Duration, Instant},
};

/// Forget addresses that have not connected for a while once there are this many of them.
const PRUNE_AT: usize = 1024;

pub struct RateLimit {
    max: usize,
    window: Duration,
    hits: HashMap<IpAddr, VecDeque<Instant>>,
}

impl RateLimit {
    /// At most `max` connections from one address within `window`.
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            max,
            window,
            hits: HashMap::new(),
        }
    }

    /// Records an attempt and says whether it is allowed. Refused attempts do not count, so a client
    /// is let in again once its old attempts leave the window.
    pub fn allow(&mut self, ip: IpAddr, now: Instant) -> bool {
        if self.hits.len() >= PRUNE_AT {
            let window = self.window;
            self.hits.retain(|_, times| {
                times
                    .back()
                    .is_some_and(|t| now.duration_since(*t) < window)
            });
        }
        let times = self.hits.entry(ip).or_default();
        while times
            .front()
            .is_some_and(|t| now.duration_since(*t) >= self.window)
        {
            times.pop_front();
        }
        if times.len() >= self.max {
            return false;
        }
        times.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn blocks_after_the_maximum_and_lets_in_again_later() {
        let mut limit = RateLimit::new(3, Duration::from_secs(60));
        let t0 = Instant::now();
        assert!((0..3).all(|i| limit.allow(ip(1), t0 + Duration::from_secs(i))));
        assert!(!limit.allow(ip(1), t0 + Duration::from_secs(10)));
        // The refused attempt did not extend the block: the first one leaves the window at 60 s.
        assert!(limit.allow(ip(1), t0 + Duration::from_secs(61)));
    }

    #[test]
    fn addresses_are_counted_separately() {
        let mut limit = RateLimit::new(1, Duration::from_secs(60));
        let now = Instant::now();
        assert!(limit.allow(ip(1), now));
        assert!(!limit.allow(ip(1), now));
        assert!(limit.allow(ip(2), now));
    }

    #[test]
    fn old_addresses_are_forgotten_when_there_are_many() {
        let mut limit = RateLimit::new(1, Duration::from_secs(60));
        let t0 = Instant::now();
        for n in 0..PRUNE_AT as u32 {
            limit.allow(IpAddr::from(n.to_be_bytes()), t0);
        }
        assert_eq!(limit.hits.len(), PRUNE_AT);
        limit.allow(ip(200), t0 + Duration::from_secs(120));
        assert!(limit.hits.len() < 10);
    }
}
