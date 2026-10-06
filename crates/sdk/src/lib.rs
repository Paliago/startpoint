//! UI-independent application contracts shared by the desktop host and extensions.

mod model;
mod provider;

pub use model::*;
pub use provider::*;

use async_channel::Receiver;
use std::sync::Arc;

/// The major version of the application contract, not a Rust dynamic-library ABI.
pub const API_VERSION: u32 = 2;

/// Every desktop operation goes through this interface. Submissions never wait for I/O.
pub trait ApplicationApi: Send + Sync {
    fn snapshot(&self) -> Snapshot;
    fn submit(&self, command: Command) -> Result<JobId, ApiError>;
    fn subscribe(&self) -> Receiver<AppEvent>;
    fn cancel(&self, job_id: JobId);
}

pub type Application = Arc<dyn ApplicationApi>;
