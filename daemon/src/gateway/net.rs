//! Which peers may reach the gateway, and which addresses the Mac offers to a phone.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Loopback, private and link-local ranges. The gateway is for the local network only.
pub fn is_local_range(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_local_range(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// A range the owner added, as `a.b.c.d/len`.
#[derive(Clone, Debug, PartialEq)]
pub struct Cidr {
    pub base: Ipv4Addr,
    pub len: u8,
}

impl Cidr {
    pub fn parse(text: &str) -> Option<Self> {
        let (addr, len) = text.trim().split_once('/')?;
        let len: u8 = len.parse().ok()?;
        if len > 32 {
            return None;
        }
        Some(Self { base: addr.parse().ok()?, len })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let v4 = match ip {
            IpAddr::V4(v4) => v4,
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => v4,
                None => return false,
            },
        };
        if self.len == 0 {
            return true;
        }
        let mask = u32::MAX << (32 - self.len as u32);
        (u32::from(v4) & mask) == (u32::from(self.base) & mask)
    }
}

pub fn is_allowed(ip: IpAddr, extra: &[Cidr]) -> bool {
    is_local_range(ip) || extra.iter().any(|c| c.contains(ip))
}

/// The Mac's own IPv4 addresses in local ranges, loopback last. Read with `getifaddrs`.
pub fn local_addresses() -> Vec<String> {
    let mut out: Vec<Ipv4Addr> = Vec::new();
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) != 0 {
            return vec!["127.0.0.1".into()];
        }
        let mut cur = list;
        while !cur.is_null() {
            let ifa = &*cur;
            let up = ifa.ifa_flags & libc::IFF_UP as u32 != 0;
            if up && !ifa.ifa_addr.is_null() && (*ifa.ifa_addr).sa_family as i32 == libc::AF_INET {
                let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                if is_local_range(IpAddr::V4(ip)) && !out.contains(&ip) {
                    out.push(ip);
                }
            }
            cur = ifa.ifa_next;
        }
        libc::freeifaddrs(list);
    }
    out.sort_by_key(|ip| (ip.is_loopback(), ip.is_link_local(), *ip));
    if !out.iter().any(|ip| ip.is_loopback()) {
        out.push(Ipv4Addr::LOCALHOST);
    }
    out.into_iter().map(|ip| ip.to_string()).collect()
}

/// The Mac's name as people see it, for the phone's "connected to" line.
pub fn host_name() -> String {
    let mut buf = [0u8; 256];
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if rc != 0 {
        return "Mac".into();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..end]).to_string();
    name.trim_end_matches(".local").to_string()
}

#[allow(dead_code)]
pub fn unused(_: Ipv6Addr) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_local_ranges_are_allowed() {
        for ok in ["127.0.0.1", "10.0.2.2", "10.255.1.1", "172.16.0.1", "172.31.255.254", "192.168.1.20", "169.254.10.10", "::1", "fe80::1", "fd12:3456::1", "::ffff:192.168.1.5"] {
            assert!(is_allowed(ok.parse().unwrap(), &[]), "{ok} should be allowed");
        }
        for no in ["8.8.8.8", "172.32.0.1", "172.15.0.1", "192.169.0.1", "100.64.0.1", "1.1.1.1", "2001:4860:4860::8888", "::ffff:8.8.8.8"] {
            assert!(!is_allowed(no.parse().unwrap(), &[]), "{no} should be refused");
        }
        let vpn = [Cidr::parse("100.64.0.0/10").unwrap()];
        assert!(is_allowed("100.64.0.1".parse().unwrap(), &vpn));
        assert!(is_allowed("100.127.255.254".parse().unwrap(), &vpn));
        assert!(!is_allowed("100.128.0.1".parse().unwrap(), &vpn));
        assert!(Cidr::parse("1.2.3.4/33").is_none());
        assert!(Cidr::parse("nope").is_none());
    }

    #[test]
    fn the_mac_offers_at_least_loopback() {
        let addrs = local_addresses();
        assert!(addrs.contains(&"127.0.0.1".to_string()));
        assert!(addrs.iter().all(|a| is_local_range(a.parse().unwrap())));
    }
}
