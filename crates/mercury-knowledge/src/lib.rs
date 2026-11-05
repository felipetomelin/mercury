//! Local knowledge is evidence, never execution authority.
pub mod automation;
pub mod backend;
pub mod capture;
pub mod graph;
mod ingest;
mod model;
mod store;

pub use ingest::{parse_note, scan};
pub use model::*;
pub use store::Index;

use sha2::{Digest, Sha256};
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    Json(serde_json::Error),
    Invalid(&'static str),
    UnsafePath,
    Conflict,
    Cancelled,
    Unavailable,
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Underlying errors may include private paths or server text.
        f.write_str(match self {
            Self::Io(_) => "knowledge I/O failed",
            Self::Sql(_) => "knowledge storage failed",
            Self::Json(_) => "invalid model or stored JSON",
            Self::Invalid(s) => s,
            Self::UnsafePath => "unsafe knowledge path",
            Self::Conflict => "source changed; rebuild or review again",
            Self::Cancelled => "knowledge operation cancelled",
            Self::Unavailable => "knowledge model unavailable",
        })
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
