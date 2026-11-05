//! Scheduling, validation and durable storage for Mercury cron jobs. The
//! server owns execution and delivery.

pub mod executor;
pub mod job;
pub mod schedule;
pub mod scheduler;
pub mod storage;

pub use job::{CatchUp, Delivery, Job, JobTarget, Repeat};
pub use schedule::{CronSchedule, Schedule};

/// Serializes in-process scheduler mutations with reversible Learn edits of
/// jobs.toml. The scheduler's OS lock still excludes a second server.
pub fn jobs_file_lock() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .map_err(|_| "cron jobs lock is unavailable".into())
}
