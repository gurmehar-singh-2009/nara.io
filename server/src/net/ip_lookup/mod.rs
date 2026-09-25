pub mod api;
pub mod ip;
pub mod lookup;

// pub use ip::get_public_ip_addr;
pub use api::{LookupProvider, lookup};
pub use lookup::LookupResult;

pub fn normalize_ip(ip: &str) -> String {
    if let Ok(addr) = ip.parse::<std::net::IpAddr>() {
        return match addr {
            std::net::IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => v4.to_string(),
                None => addr.to_string(),
            },
            v4 => v4.to_string(),
        };
    }
    ip.to_string()
}
