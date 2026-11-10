use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() {
    let seed_file = Path::new("packet_seed.rs");

    if !seed_file.exists() {
        let seed = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
            & 0xff) as u8;

        fs::write(seed_file, format!("pub const PACKET_SEED: u8 = {seed};\n")).unwrap();
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=packet_seed.rs");
}
