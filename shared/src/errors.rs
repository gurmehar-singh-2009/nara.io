//! Contains errors that could occur.

use snafu::prelude::*;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
/// An error that could happen on both the client or the server.
pub enum SharedError {
    #[snafu(display("Failed to decode data: {source}"))]
    /// Failing to decode incoming message.
    DecodeError {
        /// The bitcode error (`bitcode::Error`).
        source: bitcode::Error,
    },

    /// Invalid byte size.
    InvalidByteSize,
}
