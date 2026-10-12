use std::net::IpAddr;

/// True for addresses in 100.64.0.0/10 or fd7a:115c:a1e0::/48.
pub fn is_tailscale(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 100 && (octets[1] & 0xC0) == 0x40
        }
        IpAddr::V6(v6) => v6.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

/// Tailscale addresses of the local interfaces.
pub fn tailscale_addrs() -> Vec<IpAddr> {
    if_addrs::get_if_addrs()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .map(|interface| interface.ip())
                .filter(|ip| is_tailscale(*ip))
                .collect()
        })
        .unwrap_or_default()
}
