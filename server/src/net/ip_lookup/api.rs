// src/api.rs

use crate::net::ip_lookup::{LookupResult, lookup::*};

#[derive(Debug, Copy, Clone)]
pub enum LookupProvider {
    // IpApi,
    IpInfo,
    IpSb,
    // IpApiIo,
    ApipCc,
    IpapiIs,
    // Geolocated,
    // IpLocationApi,
}

pub fn lookup(ip: &str, provider: LookupProvider) -> Option<LookupResult> {
    match provider {
        // LookupProvider::IpApi => lookup_with_ipapi(ip),
        LookupProvider::IpInfo => lookup_with_ipinfo(ip),
        LookupProvider::IpSb => lookup_with_ipsb(ip),
        // LookupProvider::IpApiIo => lookup_with_ipapiio(ip),
        LookupProvider::ApipCc => lookup_with_apipcc(ip),
        LookupProvider::IpapiIs => lookup_with_ipapiis(ip),
        // LookupProvider::Geolocated => lookup_with_geolocated(ip),
        // LookupProvider::IpLocationApi => lookup_with_iplocationapi(ip),
    }
}

impl LookupProvider {
    pub fn all() -> &'static [LookupProvider] {
        &[
            // LookupProvider::IpApi,
            LookupProvider::IpInfo,
            LookupProvider::IpSb,
            // LookupProvider::IpApiIo,
            LookupProvider::ApipCc,
            LookupProvider::IpapiIs,
            // LookupProvider::Geolocated,
            // LookupProvider::IpLocationApi,
        ]
    }
}
