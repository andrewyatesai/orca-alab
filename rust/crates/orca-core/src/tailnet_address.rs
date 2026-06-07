//! Tailnet IPv4 detection, ported from `src/shared/tailnet-address.ts`.
//!
//! Tailscale assigns `100.64.0.0/10` (CGNAT) addresses; phone pairing prefers
//! them over LAN addresses, which break once devices change networks.

pub fn is_tailnet_ipv4_address(address: &str) -> bool {
    let parts: Vec<&str> = address.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    let octets: Option<Vec<u32>> = parts
        .iter()
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            part.parse::<u32>().ok().filter(|&value| value <= 255)
        })
        .collect();
    match octets {
        Some(octets) => octets[0] == 100 && (64..=127).contains(&octets[1]),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_tailnet_ipv4_allocation_range() {
        assert!(is_tailnet_ipv4_address("100.64.0.1"));
        assert!(is_tailnet_ipv4_address("100.102.47.57"));
        assert!(is_tailnet_ipv4_address("100.127.255.254"));
    }

    #[test]
    fn rejects_non_tailnet_ipv4_addresses_and_malformed_input() {
        assert!(!is_tailnet_ipv4_address("100.63.255.255"));
        assert!(!is_tailnet_ipv4_address("100.128.0.1"));
        assert!(!is_tailnet_ipv4_address("192.168.1.24"));
        assert!(!is_tailnet_ipv4_address("fd7a:115c:a1e0::ce33:2f3a"));
        assert!(!is_tailnet_ipv4_address("100.102.47"));
    }
}
