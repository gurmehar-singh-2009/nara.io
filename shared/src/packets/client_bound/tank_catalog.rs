// we send this initially which contains ALL the tanks
//

use std::collections::HashMap;

use bitcode::{Decode, Encode};

use crate::{junk_packet, packets::client_bound::TankSpec};

junk_packet! {
    #[allow(unused)]
    pub struct TankCatalog {
        data: HashMap<String, TankSpec>,
    }
}
