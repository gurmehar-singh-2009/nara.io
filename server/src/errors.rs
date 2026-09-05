use snafu::prelude::*;
use tokio::sync::mpsc::error::SendError;

use crate::fs::load_config::ConfigError;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum ServerError {
    #[snafu(display("Failed to push packet to send channel for {id}"))]
    ChannelEntryError { id: u32, source: SendError<Vec<u8>> },

    #[snafu(display("Failed to encrypt packet"))]
    CipherEncrypt { source: chacha20poly1305::Error },

    #[snafu(display("Cipher not initialized"))]
    CipherNotInitialized,

    #[snafu(display("Handshake failure"))]
    HandshakeFailure,

    #[snafu(display("Error converting slice to chacha20poly1305 key type: {source}"))]
    ConvertingSliceToKeyTypeError { source: ! },

    #[snafu(display("HKDF Expansion failure during handshake exchange"))]
    HKDFExpansionFailure { source: hkdf::InvalidLength },

    #[snafu(display("Error retrieving system time"))]
    SystemTimeError { source: std::time::SystemTimeError },

    #[snafu(display("Failed to load internal configs file: {source}"))]
    LoadConfigError { source: ConfigError },

    #[snafu(display("Failed to bind to port"))]
    PortBindFailure { source: std::io::Error },
}
