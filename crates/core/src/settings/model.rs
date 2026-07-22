use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default = "current_app_version")]
    pub app_version: String,
    #[serde(default)]
    pub general: GeneralSettings,
    #[serde(default)]
    pub appearance: AppearanceSettings,
    #[serde(default)]
    pub processing: ProcessingDefaults,
    #[serde(default)]
    pub export: ExportDefaults,
    #[serde(default)]
    pub canvas_size: CanvasSizeDefaults,
    #[serde(default)]
    pub window: WindowState,
    #[serde(default)]
    pub recent: RecentFiles,
    #[serde(default)]
    pub updates: UpdateSettings,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSettings {
    /// Check for and download updates in the background. A verified update is
    /// applied only after PlotX closes.
    #[serde(default = "default_auto_check")]
    pub auto_check: bool,
    /// Which release train to follow; `Auto` follows the build's own channel.
    #[serde(default)]
    pub channel: crate::update::UpdateChannelSetting,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            auto_check: default_auto_check(),
            channel: Default::default(),
        }
    }
}

fn default_auto_check() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeneralSettings {
    #[serde(default = "default_snap_enabled")]
    pub snap_enabled: bool,
    /// Number of complete previous project files retained beside the project.
    /// Zero disables save-time backups; crash-recovery snapshots are separate.
    #[serde(default = "default_project_backup_generations")]
    pub project_backup_generations: u8,
}

pub const MAX_PROJECT_BACKUP_GENERATIONS: u8 = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct AppearanceSettings {
    #[serde(default)]
    pub theme: ThemeMode,
    #[serde(default)]
    pub ui_scale: UiScaleSettings,
    #[serde(default)]
    pub graphics_power: GraphicsPowerPreference,
    /// Optional editor-chrome accent. Figure colours and exports are unaffected.
    #[serde(default)]
    pub canvas_accent: Option<[u8; 3]>,
}

/// GPU adapter class requested at the next application start. The platform may
/// still choose another compatible adapter when the preferred class is absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsPowerPreference {
    #[default]
    LowPower,
    HighPerformance,
}

impl GraphicsPowerPreference {
    pub const ALL: [Self; 2] = [Self::LowPower, Self::HighPerformance];

    pub const fn label(self) -> &'static str {
        match self {
            Self::LowPower => "Power saving (integrated GPU)",
            Self::HighPerformance => "High performance (discrete GPU)",
        }
    }
}

/// Per-monitor UI scale records, keyed by a stable identity of the display
/// (device name plus native resolution and physical size, so replacing the
/// panel or changing its mode mints a fresh entry and a fresh auto value).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct UiScaleSettings {
    #[serde(default)]
    pub monitors: std::collections::BTreeMap<String, MonitorScale>,
}

/// The UI zoom for one monitor: the automatically derived legible value plus
/// an optional manual override (Ctrl+= / Ctrl+- or the Preferences slider).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonitorScale {
    /// Zoom computed from the display's physical density on first sight.
    pub auto: f32,
    /// Manual override; `None` follows `auto`.
    #[serde(default)]
    pub user: Option<f32>,
}

impl MonitorScale {
    pub fn effective(&self) -> f32 {
        self.user.unwrap_or(self.auto)
    }
}

/// The application chrome theme, kept correct-by-construction as an enum so an
/// invalid value can't be stored. An unrecognized string in a settings file
/// falls back to `System`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
    #[serde(other)]
    #[default]
    System,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 3] = [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark];

    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::System => "Follow system",
            ThemeMode::Light => "Light",
            ThemeMode::Dark => "Dark",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProcessingDefaults {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportDefaults {
    #[serde(default)]
    pub include_view_snapshots: bool,
    #[serde(default = "default_export_dpi")]
    pub dpi: u16,
    #[serde(default)]
    pub trim_to_visible_content: bool,
}

/// Sticky choices of the canvas-size popover.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct CanvasSizeDefaults {
    /// When true, applying a new size also scales the page content uniformly
    /// by the width ratio (font sizes stay physical).
    #[serde(default)]
    pub scale_content: bool,
    /// Most recently applied preset ids, newest first.
    #[serde(default)]
    pub recent_presets: Vec<String>,
    /// User-saved custom page sizes.
    #[serde(default)]
    pub custom_presets: Vec<CustomSizePreset>,
}

/// Upper bound for `CanvasSizeDefaults::recent_presets`, enforced on every
/// note so a hand-edited settings file cannot grow the list without bound.
pub const MAX_RECENT_SIZE_PRESETS: usize = 5;

impl CanvasSizeDefaults {
    /// Move `id` to the front of the recent list, deduplicating and truncating
    /// to [`MAX_RECENT_SIZE_PRESETS`].
    pub fn note_recent(&mut self, id: &str) {
        self.recent_presets.retain(|existing| existing != id);
        self.recent_presets.insert(0, id.to_owned());
        self.recent_presets.truncate(MAX_RECENT_SIZE_PRESETS);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomSizePreset {
    pub name: String,
    pub width_mm: f32,
    pub height_mm: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct WindowState {
    #[serde(default)]
    pub main: Option<WindowGeometry>,
    #[serde(default)]
    pub last_open_directory: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RecentFiles {
    #[serde(default)]
    pub files: Vec<PathBuf>,
}

/// Upper bound for `RecentFiles::files`; enforced on every note and load so a
/// hand-edited settings file cannot grow the list without bound.
pub const MAX_RECENT_FILES: usize = 10;

impl RecentFiles {
    /// Move `path` to the front of the list, deduplicating and truncating to
    /// [`MAX_RECENT_FILES`].
    pub fn note(&mut self, path: PathBuf) {
        self.files.retain(|existing| *existing != path);
        self.files.insert(0, path);
        self.files.truncate(MAX_RECENT_FILES);
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: current_schema_version(),
            app_version: current_app_version(),
            general: GeneralSettings::default(),
            appearance: AppearanceSettings::default(),
            processing: ProcessingDefaults::default(),
            export: ExportDefaults::default(),
            canvas_size: CanvasSizeDefaults::default(),
            window: WindowState::default(),
            recent: RecentFiles::default(),
            updates: UpdateSettings::default(),
        }
    }
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            snap_enabled: default_snap_enabled(),
            project_backup_generations: default_project_backup_generations(),
        }
    }
}

impl Default for ExportDefaults {
    fn default() -> Self {
        Self {
            include_view_snapshots: false,
            dpi: default_export_dpi(),
            trim_to_visible_content: false,
        }
    }
}

fn current_schema_version() -> u32 {
    super::SETTINGS_SCHEMA_VERSION
}

fn current_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

fn default_snap_enabled() -> bool {
    true
}

fn default_project_backup_generations() -> u8 {
    1
}

fn default_export_dpi() -> u16 {
    crate::export::DEFAULT_BITMAP_DPI
}
