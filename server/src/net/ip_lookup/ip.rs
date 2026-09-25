// src/ip.rs

// pub fn get_public_ip_addr() -> Option<String> {
//     let sources = vec![
//         ("https://icanhazip.com/", false), // plain text
//         ("https://api.ip.sb/ip", false), // plain text
//         ("https://api.ipsimple.org/ipv4?format=json", true), // json {"ip": ...}
//         ("https://api.ipify.org/?format=json", true), // json {"ip": ...}
//     ];

//     let client = reqwest::blocking::Client::builder()
//         .timeout(Duration::from_secs(3))
//         .build()
//         .unwrap();

//     let mut rng = nanorand::WyRand::new();
//     let mut shuffled = sources.clone();
//     shuffled.shuffle(&mut rng);

//     for (url, is_json) in shuffled {
//         if let Ok(resp) = client.get(url).send() {
//             if is_json {
//                 if let Ok(json) = resp.json::<serde_json::Value>() {
//                     if let Some(ip) = json.get("ip").and_then(|v| v.as_str())
// {                         return Some(ip.to_string());
//                     }
//                 }
//             } else if let Ok(text) = resp.text() {
//                 let ip = text.trim().to_string();
//                 if !ip.is_empty() {
//                     return Some(ip);
//                 }
//             }
//         }
//     }

//     None
// }
