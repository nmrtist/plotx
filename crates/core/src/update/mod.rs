//! Application self-update: release channels, the update-check/download
//! protocol, and the background service polled from the frame loop.
//!
//! Release metadata comes directly from GitHub. Tests redirect requests to
//! self-contained local servers with `PLOTX_UPDATE_URL`.

mod channel;
mod install;
mod protocol;
mod service;

pub use channel::{UpdateChannel, UpdateChannelSetting};
pub use install::{InstallPlan, cleanup_after_restart, run_helper_from_args};
pub use protocol::{DEFAULT_SERVER_URL, ReleaseAsset, UpdateError, server_url};
pub use service::{UpdateService, UpdateStatus};

/// True when the running binary is a newer version than `previous` (the
/// version that last wrote the settings file) — i.e. this is the first
/// launch after an update.
pub fn launched_after_update(previous: &str) -> bool {
    protocol::is_newer(env!("CARGO_PKG_VERSION"), previous).unwrap_or(false)
}

#[cfg(test)]
mod tests;
