use serde::{Deserialize, Serialize};

/// Release train an installed build follows for updates. Each channel only
/// ever offers its own releases; there is no cross-channel fallthrough, so an
/// alpha install never sees beta or stable builds unless the user opts in via
/// [`UpdateChannelSetting`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    Stable,
    Beta,
    Alpha,
}

impl UpdateChannel {
    /// Canonical prerelease identifier used for this channel.
    pub fn as_str(self) -> &'static str {
        match self {
            UpdateChannel::Stable => "stable",
            UpdateChannel::Beta => "beta",
            UpdateChannel::Alpha => "alpha",
        }
    }

    /// The channel this binary was built for, baked in at compile time by the
    /// release pipeline (`PLOTX_RELEASE_CHANNEL=alpha|beta|stable`). Local and
    /// unlabelled builds are treated as stable.
    pub fn built_in() -> Self {
        match option_env!("PLOTX_RELEASE_CHANNEL") {
            Some("alpha") => UpdateChannel::Alpha,
            Some("beta") => UpdateChannel::Beta,
            _ => UpdateChannel::Stable,
        }
    }
}

/// The user-facing channel preference stored in settings. `Auto` follows the
/// compile-time channel of the running build; an unrecognized value in a
/// settings file falls back to `Auto` rather than failing the whole parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannelSetting {
    Stable,
    Beta,
    Alpha,
    #[serde(other)]
    #[default]
    Auto,
}

impl UpdateChannelSetting {
    pub const ALL: [UpdateChannelSetting; 4] = [
        UpdateChannelSetting::Auto,
        UpdateChannelSetting::Stable,
        UpdateChannelSetting::Beta,
        UpdateChannelSetting::Alpha,
    ];

    pub fn resolve(self) -> UpdateChannel {
        match self {
            UpdateChannelSetting::Auto => UpdateChannel::built_in(),
            UpdateChannelSetting::Stable => UpdateChannel::Stable,
            UpdateChannelSetting::Beta => UpdateChannel::Beta,
            UpdateChannelSetting::Alpha => UpdateChannel::Alpha,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            UpdateChannelSetting::Auto => match UpdateChannel::built_in() {
                UpdateChannel::Stable => "Follow build (stable)",
                UpdateChannel::Beta => "Follow build (beta)",
                UpdateChannel::Alpha => "Follow build (alpha)",
            },
            UpdateChannelSetting::Stable => "Stable",
            UpdateChannelSetting::Beta => "Beta",
            UpdateChannelSetting::Alpha => "Alpha",
        }
    }
}
