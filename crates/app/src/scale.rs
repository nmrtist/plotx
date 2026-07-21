//! Adaptive per-monitor UI scale.
//!
//! egui sizes everything in logical points; with OS display scaling switched
//! off (a common choice on high-density Windows monitors) one point is one
//! physical pixel and the whole UI shrinks physically. This module derives a
//! default zoom per monitor from two legibility constraints — body text must
//! be at least [`MIN_EM_MM`] tall physically and at least [`MIN_EM_PX`] pixels
//! tall — and applies it through egui's global `zoom_factor`. The derived
//! value never goes below 1.0, so whenever the OS scale (or a low-density
//! display) already renders text large enough the whole system is a no-op.
//!
//! Each monitor is recorded once in settings under a stable identity key; the
//! user can override the value there (Preferences or Ctrl+= / Ctrl+- / Ctrl+0)
//! and the override persists per monitor. The driver probes the monitor under
//! the window every frame (cheap) and re-applies the recorded zoom whenever
//! the window lands on a different display.

use plotx_core::settings::{self, MonitorScale};
use plotx_core::state::{MonitorScaleStatus, PlotxApp};

/// egui's default `Body` text size in points; the em everything is judged by.
const BODY_PT: f32 = 12.5;
/// Minimum physical body-text height. At a ~55 cm desktop viewing distance
/// 3.0 mm subtends ≈0.31°, above the ~0.25° reading-comfort floor, while
/// staying denser than the OS-recommended parity (~3.3 mm at an effective
/// 96 DPI) for users who prefer compact UIs.
const MIN_EM_MM: f32 = 3.0;
/// Minimum body-text height in physical pixels; below ~12 px anti-aliased
/// glyphs (and the 0.8× "small" style derived from them) lose legibility.
const MIN_EM_PX: f32 = 12.0;
/// Automatic zoom is only ever an upscale and stays within sane bounds.
const AUTO_RANGE: std::ops::RangeInclusive<f32> = 1.0..=3.0;
/// Manual overrides may go denser than automatic ever would.
pub(crate) const USER_RANGE: std::ops::RangeInclusive<f32> = 0.75..=3.0;
/// Ctrl+= / Ctrl+- step.
const NUDGE_STEP: f32 = 0.10;

/// Zoom values snap to 5% so persisted numbers stay tidy.
fn snap(zoom: f32) -> f32 {
    (zoom * 20.0).round() / 20.0
}

/// The dual-constraint automatic zoom for a display of `ppi` physical pixels
/// per inch (`None` when the display did not report its size) at the OS scale
/// factor `native_ppp`.
fn auto_zoom(ppi: Option<f32>, native_ppp: f32) -> f32 {
    let native_em_px = BODY_PT * native_ppp.max(0.5);
    let physical_need_px = ppi.map_or(0.0, |ppi| MIN_EM_MM / 25.4 * ppi);
    let need_px = physical_need_px.max(MIN_EM_PX);
    let raw = need_px / native_em_px;
    // Corrections under 10% trade fractional-scale rendering for no percept-
    // ible legibility gain (e.g. a 27" QHD at 109 PPI computes 1.03); stay
    // at identity inside that dead-band.
    if raw < 1.1 {
        return 1.0;
    }
    snap(raw.clamp(*AUTO_RANGE.start(), *AUTO_RANGE.end()))
}

/// What the platform probe learned about the monitor under the window.
struct MonitorProbe {
    /// Stable settings key for this display in this mode.
    key: String,
    /// Physical pixels per inch, when the display reported its dimensions.
    ppi: Option<f32>,
    /// Cheap identity compared every frame to detect monitor changes.
    token: u64,
}

pub struct ScaleDriver {
    /// Inert when the screenshot harness owns the zoom.
    enabled: bool,
    /// `token` of the monitor handled last frame.
    current: Option<u64>,
    /// Once after startup, regrow the window to its intended logical size on
    /// the frame where the new zoom is live (resizing on the same frame as a
    /// zoom change races inside eframe; see the shot harness notes).
    pending_resize: Option<f32>,
}

impl ScaleDriver {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            current: None,
            pending_resize: None,
        }
    }

    pub fn drive(&mut self, app: &mut PlotxApp, ctx: &egui::Context, frame: &eframe::Frame) {
        if !self.enabled {
            return;
        }
        self.finish_pending_resize(ctx);
        let Some(probe) = probe_monitor(ctx, frame) else {
            return;
        };
        if !Self::monitor_needs_probe(self.current, probe.token, app.session.monitor.is_some()) {
            return;
        }

        let native_ppp = ctx
            .input(|i| i.viewport().native_pixels_per_point)
            .unwrap_or(1.0);
        let auto = auto_zoom(probe.ppi, native_ppp);
        let mut scale = MonitorScale { auto, user: None };
        let save_result = settings::try_update(|settings| {
            let entry = settings
                .appearance
                .ui_scale
                .monitors
                .entry(probe.key.clone())
                .or_insert(scale);
            // Refresh `auto` on every sight: same key means same hardware, but
            // the derivation itself may have changed between app versions.
            entry.auto = auto;
            scale = *entry;
        });

        let first_sight = self.current.is_none();
        self.current = Some(probe.token);
        app.session.monitor = Some(MonitorScaleStatus {
            key: probe.key,
            auto: scale.auto,
            user: scale.user,
            ppi: probe.ppi,
        });
        if let Err(error) = save_result {
            app.session.status = format!("Could not save display scale settings: {error}");
        }
        let zoom = scale.effective();
        if (ctx.zoom_factor() - zoom).abs() > f32::EPSILON {
            ctx.set_zoom_factor(zoom);
            if first_sight && zoom > 1.0 {
                // The window was created in pre-zoom points, so its logical
                // content area just shrank by `zoom`; restore it next frame.
                self.pending_resize = Some(zoom);
            }
        }
    }

    fn finish_pending_resize(&mut self, ctx: &egui::Context) {
        let Some(zoom) = self.pending_resize else {
            return;
        };
        if (ctx.zoom_factor() - zoom).abs() > f32::EPSILON {
            return; // the new zoom is not live yet
        }
        self.pending_resize = None;
        let mut size = egui::vec2(crate::DEFAULT_WINDOW_PT[0], crate::DEFAULT_WINDOW_PT[1]);
        if let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) {
            // `monitor_size` is already in the new logical points; keep the
            // window comfortably inside the display.
            size = size.min(monitor * 0.92);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }

    fn monitor_needs_probe(current: Option<u64>, token: u64, app_has_monitor: bool) -> bool {
        current != Some(token) || !app_has_monitor
    }
}

/// Shift the current monitor's manual override by `steps` nudges and persist
/// it. An override landing exactly on the automatic value clears itself so the
/// monitor goes back to following `auto`.
pub(crate) fn nudge_ui_zoom(app: &mut PlotxApp, ctx: &egui::Context, steps: i32) {
    let Some(status) = app.session.monitor.clone() else {
        return;
    };
    let target = snap(status.effective() + NUDGE_STEP * steps as f32)
        .clamp(*USER_RANGE.start(), *USER_RANGE.end());
    let user = (target != status.auto).then_some(target);
    set_ui_zoom(app, ctx, user);
}

pub(crate) fn reset_ui_zoom(app: &mut PlotxApp, ctx: &egui::Context) {
    set_ui_zoom(app, ctx, None);
}

fn set_ui_zoom(app: &mut PlotxApp, ctx: &egui::Context, user: Option<f32>) {
    set_ui_zoom_with(app, ctx, user, persist_ui_zoom);
}

fn set_ui_zoom_with(
    app: &mut PlotxApp,
    ctx: &egui::Context,
    user: Option<f32>,
    persist: impl FnOnce(&str, f32, Option<f32>) -> std::io::Result<()>,
) {
    let Some(status) = app.session.monitor.as_ref() else {
        return;
    };
    let key = status.key.clone();
    let auto = status.auto;
    let save_result = persist(&key, auto, user);
    let status = app.session.monitor.as_mut().expect("status checked above");
    status.user = user;
    let zoom = status.effective();
    ctx.set_zoom_factor(zoom);
    app.session.status = match (user, save_result) {
        (Some(_), Ok(())) => format!("UI scale {:.0}% on this display.", zoom * 100.0),
        (None, Ok(())) => {
            format!("UI scale automatic ({:.0}%) on this display.", zoom * 100.0)
        }
        (_, Err(error)) => format!(
            "UI scale changed to {:.0}% for this session, but could not save it: {error}",
            zoom * 100.0
        ),
    };
}

fn persist_ui_zoom(key: &str, auto: f32, user: Option<f32>) -> std::io::Result<()> {
    settings::try_update(|settings| {
        update_monitor_override(settings, key, auto, user);
    })
}

fn update_monitor_override(
    settings: &mut settings::Settings,
    key: &str,
    auto: f32,
    user: Option<f32>,
) {
    settings
        .appearance
        .ui_scale
        .monitors
        .entry(key.to_owned())
        .or_insert(MonitorScale { auto, user: None })
        .user = user;
}

#[cfg(windows)]
fn probe_monitor(_ctx: &egui::Context, frame: &eframe::Frame) -> Option<MonitorProbe> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Gdi::{
        CreateDCW, DeleteDC, GetDeviceCaps, GetMonitorInfoW, HORZRES, HORZSIZE,
        MONITOR_DEFAULTTONEAREST, MONITORINFOEXW, MonitorFromWindow, VERTRES, VERTSIZE,
    };

    let handle = frame.window_handle().ok()?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return None;
    };
    let hwnd = win32.hwnd.get() as *mut core::ffi::c_void;
    // SAFETY: hwnd comes from the live winit window; MonitorFromWindow only
    // reads it and returns a monitor handle that needs no release.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_null() {
        return None;
    }

    let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: `info` is a properly sized MONITORINFOEXW and cbSize is set.
    if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) } == 0 {
        return None;
    }
    let device_end = info
        .szDevice
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(info.szDevice.len());
    let device = String::from_utf16_lossy(&info.szDevice[..device_end]);

    // A monitor DC reports the display's true pixel mode (the process is
    // per-monitor DPI aware) and its EDID physical size in millimetres.
    let display: Vec<u16> = "DISPLAY\0".encode_utf16().collect();
    // SAFETY: both name buffers are NUL-terminated UTF-16; the DC is released
    // below on every path.
    let dc = unsafe {
        CreateDCW(
            display.as_ptr(),
            info.szDevice.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if dc.is_null() {
        return None;
    }
    // SAFETY: dc is a valid DC from CreateDCW; GetDeviceCaps only reads it.
    let (px_w, px_h, mm_w, mm_h) = unsafe {
        (
            GetDeviceCaps(dc, HORZRES as i32),
            GetDeviceCaps(dc, VERTRES as i32),
            GetDeviceCaps(dc, HORZSIZE as i32),
            GetDeviceCaps(dc, VERTSIZE as i32),
        )
    };
    // SAFETY: dc came from CreateDCW above and is released exactly once.
    unsafe { DeleteDC(dc) };

    let ppi = (mm_w > 0 && px_w > 0)
        .then(|| px_w as f32 / (mm_w as f32 / 25.4))
        // Virtual machines and projectors report junk EDID sizes; outside a
        // plausible density fall back to "unknown" rather than a wild zoom.
        .filter(|ppi| (60.0..=400.0).contains(ppi));
    Some(MonitorProbe {
        key: format!("{device} {px_w}x{px_h} {mm_w}x{mm_h}mm"),
        ppi,
        token: monitor as u64,
    })
}

/// macOS and Wayland/X11 report a trustworthy scale factor, so the physical
/// probe is Windows-only; elsewhere the automatic zoom is 1.0 (no upscale) and
/// the per-monitor manual override still works, keyed by the display mode.
#[cfg(not(windows))]
fn probe_monitor(ctx: &egui::Context, _frame: &eframe::Frame) -> Option<MonitorProbe> {
    let size = ctx.input(|i| i.viewport().monitor_size)?;
    let (px_w, px_h) = physical_monitor_size(size, ctx.pixels_per_point())?;
    Some(MonitorProbe {
        key: format!("monitor {px_w}x{px_h}"),
        ppi: None,
        token: (px_w as u64) << 32 | px_h as u64,
    })
}

#[cfg(any(not(windows), test))]
fn physical_monitor_size(size: egui::Vec2, pixels_per_point: f32) -> Option<(u32, u32)> {
    if !size.x.is_finite()
        || !size.y.is_finite()
        || !pixels_per_point.is_finite()
        || size.x <= 0.0
        || size.y <= 0.0
        || pixels_per_point <= 0.0
    {
        return None;
    }
    Some((
        (size.x * pixels_per_point).round() as u32,
        (size.y * pixels_per_point).round() as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_zoom_is_identity_on_standard_density_displays() {
        // 24" 1920×1080 ≈ 92 PPI at 100% OS scale: already legible.
        assert_eq!(auto_zoom(Some(92.0), 1.0), 1.0);
        // 27" 2560×1440 ≈ 109 PPI computes a 1.03 correction — inside the
        // dead-band, so it stays at identity like Windows recommends.
        assert_eq!(auto_zoom(Some(109.0), 1.0), 1.0);
    }

    #[test]
    fn auto_zoom_upscales_dense_displays_with_os_scaling_off() {
        // ~144 PPI panel (Windows would recommend 150%) at forced 100%:
        // 3.0 mm / 25.4 × 144 = 17.0 px needed over a 12.5 px em.
        assert_eq!(auto_zoom(Some(144.0), 1.0), 1.35);
        // 4K 13" laptop ≈ 339 PPI forced to 100% clamps to the ceiling.
        assert_eq!(auto_zoom(Some(339.0), 1.0), 3.0);
    }

    #[test]
    fn auto_zoom_defers_to_a_sufficient_os_scale() {
        // The same 144 PPI panel with the recommended 150% OS scale already
        // renders 18.75 px ems; no extra zoom on top.
        assert_eq!(auto_zoom(Some(144.0), 1.5), 1.0);
        // Unknown physical size: trust the OS scale outright.
        assert_eq!(auto_zoom(None, 1.0), 1.0);
        assert_eq!(auto_zoom(None, 2.0), 1.0);
    }

    #[test]
    fn snap_lands_on_five_percent_steps() {
        assert_eq!(snap(1.3601), 1.35);
        assert_eq!(snap(1.374), 1.35);
        assert_eq!(snap(1.376), 1.4);
    }

    #[test]
    fn physical_monitor_identity_does_not_change_with_ui_zoom() {
        let native = physical_monitor_size(egui::vec2(2560.0, 1440.0), 1.0);
        let zoomed = physical_monitor_size(egui::vec2(2560.0 / 1.5, 1440.0 / 1.5), 1.5);
        assert_eq!(native, Some((2560, 1440)));
        assert_eq!(zoomed, native);
    }

    #[test]
    fn missing_application_monitor_state_is_rehydrated() {
        let token = 42;
        assert!(ScaleDriver::monitor_needs_probe(Some(token), token, false));
        assert!(!ScaleDriver::monitor_needs_probe(Some(token), token, true));
    }

    #[test]
    fn failed_override_save_is_visible_and_keeps_session_zoom() {
        let mut app = PlotxApp::new_with_settings(settings::Settings::default());
        app.session.monitor = Some(MonitorScaleStatus {
            key: "test-monitor".to_owned(),
            auto: 1.0,
            user: None,
            ppi: None,
        });
        let ctx = egui::Context::default();
        set_ui_zoom_with(&mut app, &ctx, Some(1.5), |_, _, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "read-only settings",
            ))
        });

        assert_eq!(app.session.monitor.as_ref().unwrap().user, Some(1.5));
        assert!(app.session.status.contains("could not save it"));
        assert!(app.session.status.contains("read-only settings"));
    }

    #[test]
    fn override_recreates_a_monitor_record_missing_from_disk() {
        let mut settings = settings::Settings::default();
        update_monitor_override(&mut settings, "test-monitor", 1.35, Some(1.5));

        assert_eq!(
            settings.appearance.ui_scale.monitors["test-monitor"],
            MonitorScale {
                auto: 1.35,
                user: Some(1.5),
            }
        );
    }
}
