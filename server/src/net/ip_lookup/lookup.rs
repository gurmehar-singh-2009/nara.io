// src/lookup.rs

use serde::Serialize;

#[derive(Serialize, Debug, Default)]
pub struct CountryInfo {
    pub city: Option<String>,
    pub code: Option<String>,
    pub zip: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Serialize, Debug, Default)]
pub struct LocationInfo {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Serialize, Debug, Default)]
pub struct ConnectionInfo {
    pub is_proxy: Option<bool>,
    pub is_tor: Option<bool>,
    pub is_crawler: Option<bool>,
    pub is_datacenter: Option<bool>,
    pub is_vpn: Option<bool>,
}

#[derive(Serialize, Debug, Default)]
pub struct NetworkInfo {
    pub ip: Option<String>,
    pub isp: Option<String>,
    pub org: Option<String>,
    pub asn: Option<String>,
}

#[derive(Serialize, Debug, Default)]
pub struct LookupResult {
    pub country: CountryInfo,
    pub location: LocationInfo,
    pub connection: ConnectionInfo,
    pub network: NetworkInfo,
}

pub fn lookup_with_ipinfo(ip: &str) -> Option<LookupResult> {
    let url = format!("https://ipinfo.io/lookup/{}", ip);
    let resp = reqwest::blocking::get(url)
        .ok()?
        .json::<serde_json::Value>()
        .ok()?;

    let loc_str = resp.get("loc").and_then(|v| v.as_str()).unwrap_or("");
    let mut split = loc_str.split(',');
    let latitude = split.next().and_then(|v| v.parse().ok());
    let longitude = split.next().and_then(|v| v.parse().ok());

    Some(LookupResult {
        country: CountryInfo {
            city: resp.get("city").and_then(|v| v.as_str()).map(String::from),
            code: resp
                .get("country")
                .and_then(|v| v.as_str())
                .map(String::from),
            zip: resp
                .get("postal")
                .and_then(|v| v.as_str())
                .map(String::from),
            timezone: resp
                .get("timezone")
                .and_then(|v| v.as_str())
                .map(String::from),
        },
        location: LocationInfo {
            latitude,
            longitude,
        },
        connection: ConnectionInfo::default(),
        network: NetworkInfo {
            ip: resp.get("ip").and_then(|v| v.as_str()).map(String::from),
            isp: resp.get("org").and_then(|v| v.as_str()).map(String::from),
            org: resp.get("org").and_then(|v| v.as_str()).map(String::from),
            asn: None,
        },
    })
}

// pub fn lookup_with_ipapi(ip: &str) -> Option<LookupResult> {
//     let url = format!("http://ip-api.com/json/{}", ip);
//     let resp =
// reqwest::blocking::get(&url).ok()?.json::<serde_json::Value>().ok()?;

//     if resp.get("status")?.as_str()? != "success" {
//         return None;
//     }

//     Some(LookupResult {
//         country: CountryInfo {
//             city: resp.get("city").and_then(|v|
// v.as_str()).map(String::from),             code:
// resp.get("countryCode").and_then(|v| v.as_str()).map(String::from),
//             zip: resp.get("zip").and_then(|v| v.as_str()).map(String::from),
//             timezone: resp.get("timezone").and_then(|v|
// v.as_str()).map(String::from),         },
//         location: LocationInfo {
//             latitude: resp.get("lat").and_then(|v| v.as_f64()),
//             longitude: resp.get("lon").and_then(|v| v.as_f64()),
//         },
//         connection: ConnectionInfo::default(),
//         network: NetworkInfo {
//             ip: resp.get("query").and_then(|v| v.as_str()).map(String::from),
//             isp: resp.get("isp").and_then(|v| v.as_str()).map(String::from),
//             org: resp.get("org").and_then(|v| v.as_str()).map(String::from),
//             asn: resp.get("as").and_then(|v| v.as_str()).map(String::from),
//         },
//     })
// }

pub fn lookup_with_ipsb(ip: &str) -> Option<LookupResult> {
    let url = format!("https://api.ip.sb/geoip/{}", ip);
    let resp = reqwest::blocking::get(&url)
        .ok()?
        .json::<serde_json::Value>()
        .ok()?;

    Some(LookupResult {
        country: CountryInfo {
            city: resp.get("city").and_then(|v| v.as_str()).map(String::from),
            code: resp
                .get("country_code")
                .and_then(|v| v.as_str())
                .map(String::from),
            zip: resp
                .get("postal_code")
                .and_then(|v| v.as_str())
                .map(String::from),
            timezone: resp
                .get("timezone")
                .and_then(|v| v.as_str())
                .map(String::from),
        },
        location: LocationInfo {
            latitude: resp.get("latitude").and_then(|v| v.as_f64()),
            longitude: resp.get("longitude").and_then(|v| v.as_f64()),
        },
        connection: ConnectionInfo::default(),
        network: NetworkInfo {
            ip: resp.get("ip").and_then(|v| v.as_str()).map(String::from),
            isp: resp.get("isp").and_then(|v| v.as_str()).map(String::from),
            org: resp
                .get("organization")
                .and_then(|v| v.as_str())
                .map(String::from),
            asn: resp.get("asn").map(|v| v.to_string()),
        },
    })
}

// pub fn lookup_with_ipapiio(ip: &str) -> Option<LookupResult> {
//     let url = format!("https://ip-api.io/api/v1/ip/{}", ip);
//     let resp =
// reqwest::blocking::get(url).ok()?.json::<serde_json::Value>().ok()?;

//     let location = resp.get("location")?;
//     let factors =
// resp.get("suspicious_factors").unwrap_or(&serde_json::Value::Null);

//     Some(LookupResult {
//         country: CountryInfo {
//             city: location.get("city").and_then(|v|
// v.as_str()).map(String::from),             code:
// location.get("country_code").and_then(|v| v.as_str()).map(String::from),
//             zip: location.get("zip").and_then(|v|
// v.as_str()).map(String::from),             timezone:
// location.get("timezone").and_then(|v| v.as_str()).map(String::from),
//         },
//         location: LocationInfo {
//             latitude: location.get("latitude").and_then(|v| v.as_f64()),
//             longitude: location.get("longitude").and_then(|v| v.as_f64()),
//         },
//         connection: ConnectionInfo {
//             is_proxy: factors.get("is_proxy").and_then(|v| v.as_bool()),
//             is_tor: factors.get("is_tor_node").and_then(|v| v.as_bool()),
//             is_crawler: factors.get("is_crawler").and_then(|v| v.as_bool()),
//             is_datacenter: factors.get("is_datacenter").and_then(|v|
// v.as_bool()),             is_vpn: factors.get("is_vpn").and_then(|v|
// v.as_bool()),         },
//         network: NetworkInfo {
//             ip: resp.get("ip").and_then(|v| v.as_str()).map(String::from),
//             isp: None,
//             org: None,
//             asn: None,
//         },
//     })
// }

pub fn lookup_with_apipcc(ip: &str) -> Option<LookupResult> {
    let url = format!("https://apip.cc/api-json/{}", ip);
    let resp = reqwest::blocking::get(&url)
        .ok()?
        .json::<serde_json::Value>()
        .ok()?;

    if resp.get("status")?.as_str()? != "success" {
        return None;
    }

    Some(LookupResult {
        country: CountryInfo {
            city: resp.get("City").and_then(|v| v.as_str()).map(String::from),
            code: resp
                .get("CountryCode")
                .and_then(|v| v.as_str())
                .map(String::from),
            zip: resp
                .get("Postal")
                .and_then(|v| v.as_str())
                .map(String::from),
            timezone: resp
                .get("TimeZone")
                .and_then(|v| v.as_str())
                .map(String::from),
        },
        location: LocationInfo {
            latitude: resp
                .get("Latitude")
                .and_then(|v| v.as_str())
                .and_then(|v| v.parse().ok()),
            longitude: resp
                .get("Longitude")
                .and_then(|v| v.as_str())
                .and_then(|v| v.parse().ok()),
        },
        connection: ConnectionInfo::default(),
        network: NetworkInfo {
            ip: resp.get("query").and_then(|v| v.as_str()).map(String::from),
            isp: None,
            org: resp.get("org").and_then(|v| v.as_str()).map(String::from),
            asn: resp.get("asn").and_then(|v| v.as_str()).map(String::from),
        },
    })
}

pub fn lookup_with_ipapiis(ip: &str) -> Option<LookupResult> {
    let url = format!("https://api.ipapi.is/?q={}", ip);
    let resp = reqwest::blocking::get(url)
        .ok()?
        .json::<serde_json::Value>()
        .ok()?;

    let location = resp.get("location")?;
    let asn = resp.get("asn")?;
    let company = resp.get("company");

    Some(LookupResult {
        country: CountryInfo {
            city: location
                .get("city")
                .and_then(|v| v.as_str())
                .map(String::from),
            code: location
                .get("country_code")
                .and_then(|v| v.as_str())
                .map(String::from),
            zip: location
                .get("zip")
                .and_then(|v| v.as_str())
                .map(String::from),
            timezone: location
                .get("timezone")
                .and_then(|v| v.as_str())
                .map(String::from),
        },
        location: LocationInfo {
            latitude: location.get("latitude").and_then(|v| v.as_f64()),
            longitude: location.get("longitude").and_then(|v| v.as_f64()),
        },
        connection: ConnectionInfo {
            is_proxy: resp.get("is_proxy").and_then(|v| v.as_bool()),
            is_tor: resp.get("is_tor").and_then(|v| v.as_bool()),
            is_crawler: resp.get("is_crawler").and_then(|v| v.as_bool()),
            is_datacenter: resp.get("is_datacenter").and_then(|v| v.as_bool()),
            is_vpn: resp.get("is_vpn").and_then(|v| v.as_bool()),
        },
        network: NetworkInfo {
            ip: resp.get("ip").and_then(|v| v.as_str()).map(String::from),
            isp: company
                .and_then(|v| v.get("name"))
                .or_else(|| resp.get("datacenter").and_then(|v| v.get("datacenter")))
                .and_then(|v| v.as_str())
                .map(String::from),
            org: company
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
                .map(String::from),
            asn: asn.get("asn").map(|v| v.to_string()),
        },
    })
}

// pub fn lookup_with_geolocated() -> Option<LookupResult> {
//     let url = "https://us-west-1.geolocated.io/where-am-i";
//     let resp =
// reqwest::blocking::get(url).ok()?.json::<serde_json::Value>().ok()?;

//     let connection = resp.get("connection");
//     let city = resp.get("cityName").or_else(||
// resp.pointer("/country/capital"));

//     Some(LookupResult {
//         country: CountryInfo {
//             city: city.and_then(|v| v.as_str()).map(String::from),
//             code: resp.get("countryCode").and_then(|v|
// v.as_str()).map(String::from),             zip:
// resp.get("zipCode").and_then(|v| v.as_str()).map(String::from),
// timezone: resp.get("timeZone").or_else(||
// resp.pointer("/timeZoneInfo/zoneName")).and_then(|v|
// v.as_str()).map(String::from),         },
//         location: LocationInfo {
//             latitude: resp.get("latitude").and_then(|v| v.as_f64()),
//             longitude: resp.get("longitude").and_then(|v| v.as_f64()),
//         },
//         connection: ConnectionInfo::default(),
//         network: NetworkInfo {
//             ip: resp.get("ip").and_then(|v| v.as_str()).map(String::from),
//             isp: connection.and_then(|v| v.get("isp")).and_then(|v|
// v.as_str()).map(String::from),             org: connection.and_then(|v|
// v.get("organization")).and_then(|v| v.as_str()).map(String::from),
//             asn: connection.and_then(|v| v.get("asn")).map(|v|
// v.to_string()),         },
//     })
// }

// pub fn lookup_with_iplocationapi() -> Option<LookupResult> {
//     let url = "https://api.iplocationapi.net/api/self-lookup";
//     let resp =
// reqwest::blocking::get(url).ok()?.json::<serde_json::Value>().ok()?;

//     let timezone = resp.get("timezone");
//     let asn = resp.get("asn");
//     let connection = resp.get("connection");

//     Some(LookupResult {
//         country: CountryInfo {
//             city: resp.get("city").and_then(|v|
// v.as_str()).map(String::from),             code:
// resp.get("country_code").and_then(|v| v.as_str()).map(String::from),
//             zip: resp.get("postal_code").and_then(|v|
// v.as_str()).map(String::from),             timezone: timezone.and_then(|v|
// v.get("id")).and_then(|v| v.as_str()).map(String::from),         },
//         location: LocationInfo {
//             latitude: resp.get("latitude").and_then(|v| v.as_f64()),
//             longitude: resp.get("longitude").and_then(|v| v.as_f64()),
//         },
//         connection: ConnectionInfo::default(),
//         network: NetworkInfo {
//             ip: resp.get("ip").and_then(|v| v.as_str()).map(String::from),
//             isp: connection.and_then(|v| v.get("isp")).and_then(|v|
// v.as_str()).map(String::from),             org: connection.and_then(|v|
// v.get("organization")).and_then(|v| v.as_str()).map(String::from),
//             asn: asn.and_then(|v| v.get("number")).map(|v| v.to_string()),
//         },
//     })
// }
