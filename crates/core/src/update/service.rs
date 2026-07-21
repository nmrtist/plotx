use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use super::install::InstallPlan;
use super::{UpdateChannel, protocol};
use crate::settings::UpdateSettings;

/// How often the automatic check re-runs while the app stays open.
const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Where the updater currently is. `Ready` means a verified artifact is on
/// disk waiting to be installed (installation is a separate, later step).
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Downloading {
        version: String,
        percent: Option<u8>,
    },
    Ready {
        version: String,
        path: PathBuf,
    },
    /// A helper and verified executable are staged; the helper applies them
    /// only after the GUI exits.
    Installed {
        version: String,
        plan: InstallPlan,
    },
    Failed {
        message: String,
        retry_after: Option<Duration>,
    },
}

impl UpdateStatus {
    /// Short user-facing description of the current state.
    pub fn label(&self) -> String {
        match self {
            UpdateStatus::Idle => String::new(),
            UpdateStatus::Checking => "Checking for updates…".into(),
            UpdateStatus::UpToDate => "You're up to date.".into(),
            UpdateStatus::Downloading {
                version,
                percent: Some(p),
            } => format!("Downloading {version}… {p}%"),
            UpdateStatus::Downloading { version, .. } => format!("Downloading {version}…"),
            UpdateStatus::Ready { version, .. } => format!("Update {version} downloaded."),
            UpdateStatus::Installed { version, .. } => {
                format!("Update {version} ready — restart to install.")
            }
            UpdateStatus::Failed { message, .. } => format!("Update failed: {message}"),
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self,
            UpdateStatus::UpToDate
                | UpdateStatus::Ready { .. }
                | UpdateStatus::Installed { .. }
                | UpdateStatus::Failed { .. }
        )
    }
}

/// Background update checker following the same pattern as `ComputeService`:
/// a short-lived worker thread per check reports status over a channel that
/// the frame loop drains without blocking. Automatic checks fail silently
/// (back to `Idle`); manual checks surface their errors.
pub struct UpdateService {
    status: UpdateStatus,
    rx: Option<Receiver<UpdateStatus>>,
    manual: bool,
    auto_check: bool,
    channel: UpdateChannel,
    next_auto_check: Option<Instant>,
}

impl UpdateService {
    pub fn new(settings: &UpdateSettings) -> Self {
        let mut service = Self {
            status: UpdateStatus::Idle,
            rx: None,
            manual: false,
            auto_check: false,
            channel: UpdateChannel::Stable,
            next_auto_check: None,
        };
        service.configure(settings);
        service
    }

    /// Reconcile to a settings snapshot. Idempotent; safe to call on every
    /// settings edit. Enabling auto-check schedules a near-immediate check.
    pub fn configure(&mut self, settings: &UpdateSettings) {
        let channel = settings.channel.resolve();
        let channel_changed = channel != self.channel;
        self.channel = channel;
        if channel_changed {
            // Results are only valid for the channel that started the worker.
            // Dropping the receiver also makes further worker sends fail, so
            // an old download can never reach the automatic install path.
            self.rx = None;
            self.manual = false;
            if !matches!(self.status, UpdateStatus::Installed { .. }) {
                self.status = UpdateStatus::Idle;
            }
        }
        if settings.auto_check && (!self.auto_check || channel_changed) {
            // Delay slightly so app startup isn't competing with the check.
            self.next_auto_check = Some(Instant::now() + Duration::from_secs(3));
        }
        if !settings.auto_check {
            self.next_auto_check = None;
            if self.auto_check && self.rx.is_some() && !self.manual {
                // Dropping the receiver invalidates both an automatic download
                // and its background preparation result.
                self.rx = None;
                self.status = UpdateStatus::Idle;
            }
        }
        self.auto_check = settings.auto_check;
    }

    pub fn status(&self) -> &UpdateStatus {
        &self.status
    }

    pub fn is_busy(&self) -> bool {
        self.rx.is_some()
    }

    /// User-triggered check; a no-op while one is already running.
    pub fn check_now(&mut self) {
        self.start_check(true);
    }

    /// Frame-loop pump: drain worker progress and start a due automatic
    /// check. Returns true while a check/download is in flight so the caller
    /// keeps repainting.
    pub fn tick(&mut self) -> bool {
        if let Some(rx) = &self.rx {
            let mut finished = false;
            while let Ok(status) = rx.try_recv() {
                finished = status.is_terminal();
                self.status = status;
            }
            // A dropped sender without a terminal status means the worker
            // panicked; don't stay "Checking…" forever.
            if !finished
                && matches!(
                    self.rx.as_ref().map(|rx| rx.try_recv()),
                    Some(Err(std::sync::mpsc::TryRecvError::Disconnected))
                )
            {
                finished = true;
                self.status = UpdateStatus::Idle;
            }
            if finished {
                self.rx = None;
                if let UpdateStatus::Failed {
                    retry_after: Some(delay),
                    ..
                } = &self.status
                {
                    self.next_auto_check = Some(Instant::now() + *delay);
                }
                // Quietly swallow failures nobody asked about, matching the
                // offline-friendly behaviour expected of a background check.
                if !self.manual && matches!(self.status, UpdateStatus::Failed { .. }) {
                    self.status = UpdateStatus::Idle;
                }
                // Prepare a helper in background work; archive extraction and
                // copying the executable must not block the frame loop.
                if matches!(self.status, UpdateStatus::Ready { .. }) {
                    self.install();
                }
            }
        }
        if self.auto_check
            && self.rx.is_none()
            && self.next_auto_check.is_some_and(|t| Instant::now() >= t)
        {
            self.start_check(false);
        }
        self.rx.is_some()
    }

    /// Extract a downloaded update and prepare its post-exit helper.
    pub fn install(&mut self) {
        if self.rx.is_some() {
            return;
        }
        let UpdateStatus::Ready { version, path } = self.status.clone() else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        thread::spawn(move || {
            let status = match super::install::prepare(&path) {
                Ok(plan) => UpdateStatus::Installed { version, plan },
                Err(e) => UpdateStatus::Failed {
                    message: e.to_string(),
                    retry_after: e.retry_after(),
                },
            };
            if let Err(error) = tx.send(status)
                && let UpdateStatus::Installed { plan, .. } = error.0
            {
                plan.discard();
            }
        });
    }

    /// Time until the next scheduled automatic check, if any — so the shell
    /// can arrange a repaint wakeup even when the app is otherwise idle.
    pub fn next_check_delay(&self) -> Option<Duration> {
        if !self.auto_check || self.rx.is_some() {
            return None;
        }
        self.next_auto_check
            .map(|t| t.saturating_duration_since(Instant::now()))
    }

    fn start_check(&mut self, manual: bool) {
        if self.rx.is_some() {
            self.manual = self.manual || manual;
            return;
        }
        // A pending verified download stays pending; an automatic re-check
        // would only re-download the same artifact.
        if matches!(self.status, UpdateStatus::Ready { .. }) && !manual {
            return;
        }
        // Never replace a prepared installation plan with another check.
        if matches!(self.status, UpdateStatus::Installed { .. }) {
            return;
        }
        self.manual = manual;
        self.next_auto_check = Some(Instant::now() + AUTO_CHECK_INTERVAL);
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        self.status = UpdateStatus::Checking;
        let channel = self.channel;
        thread::spawn(move || run_check(channel, tx));
    }
}

/// The worker: check, compare, download, verify. Every exit path sends a
/// terminal status so the service always converges.
fn run_check(channel: UpdateChannel, tx: Sender<UpdateStatus>) {
    let send = |status: UpdateStatus| tx.send(status).is_ok();
    let installed = env!("CARGO_PKG_VERSION");
    let asset = match protocol::check_latest(&super::server_url(), channel) {
        Ok(Some(asset)) => asset,
        Ok(None) => {
            send(UpdateStatus::UpToDate);
            return;
        }
        Err(e) => {
            send(UpdateStatus::Failed {
                message: e.to_string(),
                retry_after: e.retry_after(),
            });
            return;
        }
    };
    match protocol::is_newer(&asset.version, installed) {
        Ok(true) => {}
        Ok(false) => {
            send(UpdateStatus::UpToDate);
            return;
        }
        Err(e) => {
            send(UpdateStatus::Failed {
                message: e.to_string(),
                retry_after: e.retry_after(),
            });
            return;
        }
    }
    let version = asset.version.clone();
    if !send(UpdateStatus::Downloading {
        version: version.clone(),
        percent: None,
    }) {
        return;
    }
    let result = protocol::download(&asset, |percent| {
        let _ = tx.send(UpdateStatus::Downloading {
            version: version.clone(),
            percent,
        });
    });
    match result {
        Ok(path) => {
            if !send(UpdateStatus::Ready {
                version,
                path: path.clone(),
            }) {
                let _ = path.parent().map(std::fs::remove_dir_all);
            }
        }
        Err(e) => {
            send(UpdateStatus::Failed {
                message: e.to_string(),
                retry_after: e.retry_after(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::UpdateChannelSetting;

    #[test]
    fn changing_channel_discards_in_flight_results() {
        let stable = UpdateSettings {
            auto_check: false,
            channel: UpdateChannelSetting::Stable,
        };
        let mut service = UpdateService::new(&stable);
        let (tx, rx) = std::sync::mpsc::channel();
        service.rx = Some(rx);
        service.status = UpdateStatus::Downloading {
            version: "1.0.0-alpha.1".into(),
            percent: Some(50),
        };

        service.configure(&UpdateSettings {
            auto_check: false,
            channel: UpdateChannelSetting::Alpha,
        });

        assert!(!service.is_busy());
        assert_eq!(service.status(), &UpdateStatus::Idle);
        assert!(
            tx.send(UpdateStatus::Ready {
                version: "1.0.0-alpha.1".into(),
                path: PathBuf::from("old-channel-update"),
            })
            .is_err()
        );
        service.tick();
        assert_eq!(service.status(), &UpdateStatus::Idle);
    }

    #[test]
    fn disabling_auto_check_discards_in_flight_automatic_results() {
        let mut service = UpdateService::new(&UpdateSettings {
            auto_check: true,
            channel: UpdateChannelSetting::Stable,
        });
        let (tx, rx) = std::sync::mpsc::channel();
        service.rx = Some(rx);
        service.manual = false;
        service.status = UpdateStatus::Downloading {
            version: "1.0.0".into(),
            percent: Some(50),
        };

        service.configure(&UpdateSettings {
            auto_check: false,
            channel: UpdateChannelSetting::Stable,
        });

        assert!(!service.is_busy());
        assert_eq!(service.status(), &UpdateStatus::Idle);
        assert!(
            tx.send(UpdateStatus::Ready {
                version: "1.0.0".into(),
                path: PathBuf::from("disabled-auto-update"),
            })
            .is_err()
        );
    }

    #[test]
    fn disabling_auto_check_keeps_a_manual_check_in_flight() {
        let mut service = UpdateService::new(&UpdateSettings {
            auto_check: true,
            channel: UpdateChannelSetting::Stable,
        });
        let (_tx, rx) = std::sync::mpsc::channel();
        service.rx = Some(rx);
        service.manual = true;

        service.configure(&UpdateSettings {
            auto_check: false,
            channel: UpdateChannelSetting::Stable,
        });

        assert!(service.is_busy());
    }

    #[test]
    fn automatic_rate_limit_defers_the_next_check() {
        let mut service = UpdateService::new(&UpdateSettings {
            auto_check: true,
            channel: UpdateChannelSetting::Stable,
        });
        let (tx, rx) = std::sync::mpsc::channel();
        service.rx = Some(rx);
        service.manual = false;
        tx.send(UpdateStatus::Failed {
            message: "rate limited".into(),
            retry_after: Some(Duration::from_secs(2 * 60 * 60)),
        })
        .unwrap();
        drop(tx);

        service.tick();

        assert_eq!(service.status(), &UpdateStatus::Idle);
        assert!(service.next_check_delay().unwrap() > Duration::from_secs(60 * 60));
    }
}
