//! Safe change policy for Mercury Learn. This crate is not connected to the
//! server until the reversible ledger and undo flow are complete.

mod changes;
#[cfg(unix)]
pub mod curator;
pub mod metrics;
pub mod pipeline;
pub mod policy;
pub mod reviewer;
pub mod storage;
pub mod transcript;

pub use changes::{review_snapshots, ChangeError, Changes, ReviewSnapshot};
