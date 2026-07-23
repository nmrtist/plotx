//! The eframe/egui application shell. Non-UI glue lives in the `plotx-core` crate.

// Release Windows builds are GUI apps: suppress the console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod observability;
mod scale;
mod shot;
mod ui;

use plotx_core::state::PlotxApp;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const RECOVERY_INTERVAL: Duration = Duration::from_secs(60);
const RECOVERY_RETRY_INTERVAL: Duration = Duration::from_secs(15);

/// Intended logical (point) size of a fresh main window; also the physical
/// pixel size it is created at before the UI scale is known.
pub(crate) const DEFAULT_WINDOW_PT: [f32; 2] = [1100.0, 700.0];

/// A verified update prepared by the service. It is handed to the helper only
/// after the GUI loop exits.
static PENDING_INSTALL: Mutex<Option<plotx_core::update::InstallPlan>> = Mutex::new(None);
static RELAUNCH_REQUESTED: AtomicBool = AtomicBool::new(false);
static SHOT_FAILURE: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn record_shot_failure(error: String) {
    let mut failure = SHOT_FAILURE.lock().unwrap();
    if failure.is_none() {
        *failure = Some(error);
    }
}

/// Called by the "Restart to update" / "Restart now" buttons.
pub(crate) fn request_relaunch() {
    RELAUNCH_REQUESTED.store(true, Ordering::Relaxed);
}

pub(crate) fn cancel_relaunch() {
    RELAUNCH_REQUESTED.store(false, Ordering::Relaxed);
}

struct Shell {
    app: PlotxApp,
    recovery: Option<plotx_core::project::RecoveryManager>,
    pending_recovery: Option<plotx_core::project::RecoverySnapshot>,
    pending_crash_report: Option<std::path::PathBuf>,
    recovery_job: Option<std::thread::JoinHandle<Result<(), plotx_core::project::ProjectError>>>,
    recovery_written: bool,
    next_recovery_at: Instant,
    clipboard_table_paste: ui::clipboard_table::ClipboardTablePaste,
    batch_workflow: ui::batch_workflow::AutomationUi,
    shot: Option<shot::ShotDriver>,
    scale: scale::ScaleDriver,
    #[cfg(target_os = "macos")]
    native_menu: ui::native_menu::NativeMenu,
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        observability::show_pending_crash_dialog();
        self.scale.drive(&mut self.app, &ctx, frame);
        let recovery_blocked = self.pending_recovery.is_some();
        #[cfg(target_os = "macos")]
        if !recovery_blocked {
            self.native_menu
                .poll(&mut self.app, &mut self.clipboard_table_paste, &ctx);
        }
        if !recovery_blocked && let Some(driver) = &mut self.shot {
            driver.drive(&mut self.app, &ctx);
        }
        self.show_recovery_prompt(&ctx);
        self.app.poll_compute();
        if self.app.session.updates.tick() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        } else if let Some(delay) = self.app.session.updates.next_check_delay() {
            ctx.request_repaint_after(delay.max(std::time::Duration::from_millis(100)));
        }
        if let plotx_core::update::UpdateStatus::Installed { plan, .. } =
            self.app.session.updates.status()
        {
            *PENDING_INSTALL.lock().unwrap() = Some(plan.clone());
        }
        let fitting = self.app.poll_line_fit();
        let transforming = self.app.poll_table_transform();
        ui::render(
            &mut self.app,
            &mut self.clipboard_table_paste,
            &mut self.batch_workflow,
            ui,
            recovery_blocked,
        );
        if fitting {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if transforming {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if self.app.compute_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        if self.app.data_export_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        self.tick_recovery(&ctx);
    }

    fn on_exit(&mut self) {
        if let Some(job) = self.recovery_job.take() {
            match job.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => log::error!("automatic recovery save failed on exit: {error}"),
                Err(_) => log::error!("automatic recovery worker panicked on exit"),
            }
        }
        if let Some(recovery) = self.recovery.take()
            && let Err(error) = recovery.shutdown()
        {
            log::error!("failed to clear crash-recovery snapshot on clean exit: {error}");
        }
        log::logger().flush();
    }
}

impl Shell {
    fn tick_recovery(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if self
            .recovery_job
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
        {
            let result = self
                .recovery_job
                .take()
                .expect("finished recovery worker is present")
                .join();
            match result {
                Ok(Ok(())) => {
                    self.recovery_written = true;
                    self.next_recovery_at = now + RECOVERY_INTERVAL;
                }
                Ok(Err(error)) => {
                    self.app.session.status = format!("Automatic recovery save failed: {error}");
                    self.next_recovery_at = now + RECOVERY_RETRY_INTERVAL;
                }
                Err(_) => {
                    self.app.session.status =
                        "Automatic recovery worker failed unexpectedly.".into();
                    self.next_recovery_at = now + RECOVERY_RETRY_INTERVAL;
                }
            }
        }
        if self.pending_recovery.is_some() {
            self.next_recovery_at = Instant::now() + RECOVERY_INTERVAL;
            return;
        }
        if !self.app.doc.dirty {
            if self.recovery_job.is_none() && self.recovery_written {
                match self
                    .recovery
                    .as_ref()
                    .map(plotx_core::project::RecoveryManager::clear_current)
                {
                    Some(Err(error)) => {
                        self.app.session.status =
                            format!("Saved project, but could not clear recovery data: {error}");
                        self.recovery_written = false;
                    }
                    _ => self.recovery_written = false,
                }
            }
            self.next_recovery_at = now + RECOVERY_INTERVAL;
            return;
        }
        if self.recovery_job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        if now >= self.next_recovery_at {
            if self.app.compute_busy() {
                self.next_recovery_at = now + RECOVERY_RETRY_INTERVAL;
                ctx.request_repaint_after(RECOVERY_RETRY_INTERVAL);
                return;
            }
            if let Some(recovery) = &self.recovery {
                let request = match plotx_core::project::prepare_recovery_snapshot(&self.app) {
                    Ok(request) => request,
                    Err(error) => {
                        self.app.session.status =
                            format!("Automatic recovery save failed: {error}");
                        self.next_recovery_at = now + RECOVERY_RETRY_INTERVAL;
                        ctx.request_repaint_after(RECOVERY_RETRY_INTERVAL);
                        return;
                    }
                };
                let target = recovery.target();
                self.recovery_job = Some(std::thread::spawn(move || {
                    plotx_core::project::save_recovery_snapshot(request, target)
                }));
                ctx.request_repaint_after(Duration::from_millis(100));
            } else {
                self.next_recovery_at = now + RECOVERY_INTERVAL;
            }
        }
        ctx.request_repaint_after(self.next_recovery_at.saturating_duration_since(now));
    }

    fn show_recovery_prompt(&mut self, ctx: &egui::Context) {
        let Some(snapshot) = self.pending_recovery.clone() else {
            return;
        };
        let mut recover = false;
        let mut discard = false;
        ui::modal(ctx, "recover_unsaved_project_modal", ui::ModalKind::Dialog).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.heading("Recover unsaved project");
            ui.separator();
            ui.label("PlotX found an automatic recovery snapshot left by an interrupted session.");
            if let Some(path) = &snapshot.original_path {
                ui.small(format!("Original project: {}", path.display()));
            } else {
                ui.small("The recovered document had not been saved yet.");
            }
            if let Some(path) = &self.pending_crash_report {
                ui.small(format!("Crash report: {}", path.display()));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Recover").clicked() {
                    recover = true;
                }
                if ui.button("Discard recovery").clicked() {
                    discard = true;
                }
            });
        });
        if (recover || discard) && self.pending_crash_report.take().is_some() {
            observability::acknowledge_crash_report();
        }

        if recover {
            match plotx_core::project::restore_recovery(&snapshot) {
                Ok(mut app) => {
                    let adopted = self
                        .recovery
                        .as_mut()
                        .ok_or_else(|| "recovery manager is unavailable".to_owned())
                        .and_then(|recovery| {
                            recovery.adopt(&snapshot).map_err(|error| error.to_string())
                        });
                    let cleanup_warning = match adopted {
                        Ok(warning) => warning,
                        Err(error) => {
                            self.app.session.status =
                                format!("Could not claim recovery data: {error}");
                            return;
                        }
                    };
                    app.session.status = cleanup_warning.map_or_else(
                        || "Recovered unsaved work. Save the project to make it permanent.".into(),
                        |warning| {
                            format!(
                                "Recovered unsaved work. Save the project to make it permanent. {warning}"
                            )
                        },
                    );
                    self.app = app;
                    self.pending_recovery = None;
                    self.recovery_written = true;
                    self.next_recovery_at = Instant::now() + RECOVERY_INTERVAL;
                }
                Err(error) => {
                    self.app.session.status = format!("Recovery failed: {error}");
                }
            }
        } else if discard {
            let result = self
                .recovery
                .as_mut()
                .ok_or_else(|| "recovery manager is unavailable".to_owned())
                .and_then(|recovery| {
                    recovery
                        .discard(&snapshot)
                        .map_err(|error| error.to_string())
                });
            match result {
                Ok(()) => self.pending_recovery = None,
                Err(error) => {
                    self.app.session.status = format!("Could not discard recovery data: {error}");
                }
            }
        }
    }
}

/// Undecorated windows lose the DWM frame, so Windows 11 rounded corners and
/// the drop shadow must be requested explicitly. Both calls are cosmetic:
/// failures (e.g. Windows 10 rejecting the corner attribute) are deliberately
/// ignored and the window simply stays square/shadowless.
#[cfg(windows)]
fn apply_windows_frame_polish(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea,
        DwmSetWindowAttribute,
    };
    use windows_sys::Win32::UI::Controls::MARGINS;

    let Ok(handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = win32.hwnd.get() as *mut core::ffi::c_void;
    let corner = DWMWCP_ROUND;
    // SAFETY: hwnd comes from the live winit window; both DWM calls only read
    // the passed attribute structs.
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&raw const corner).cast(),
            size_of_val(&corner) as u32,
        );
        // A one-pixel frame sheet re-enables the DWM drop shadow; egui paints
        // opaque content over it, so nothing shows through.
        let margins = MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 0,
            cyBottomHeight: 1,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &raw const margins);
    }
}

fn main() -> eframe::Result<()> {
    if let Some(code) = plotx_core::update::run_helper_from_args() {
        std::process::exit(code);
    }
    observability::initialize();
    plotx_core::update::cleanup_after_restart();
    let shot_active = std::env::var_os("PLOTX_SHOT").is_some();
    let settings = plotx_core::settings::load();
    let inner = if shot_active {
        [1500.0, 1000.0]
    } else {
        DEFAULT_WINDOW_PT
    };
    #[allow(unused_mut)]
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(inner)
        .with_min_inner_size([720.0, 460.0])
        .with_title("PlotX")
        .with_icon(
            eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon-256.png"))
                .expect("embedded icon PNG is valid"),
        );
    // Windows and Linux draw a VS Code style title bar (logo + menus + window
    // controls) inside the content area; macOS keeps the native title bar and
    // system menu.
    #[cfg(not(target_os = "macos"))]
    {
        viewport = viewport.with_decorations(false);
    }
    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration {
        // Keep only one rendered frame queued so direct manipulation reaches the
        // display with the least presentation latency.
        desired_maximum_frame_latency: Some(1),
        ..Default::default()
    };
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut wgpu_options.wgpu_setup {
        setup.power_preference = match settings.appearance.graphics_power {
            plotx_core::settings::GraphicsPowerPreference::LowPower => {
                eframe::wgpu::PowerPreference::LowPower
            }
            plotx_core::settings::GraphicsPowerPreference::HighPerformance => {
                eframe::wgpu::PowerPreference::HighPerformance
            }
        };
    }
    let native_options = eframe::NativeOptions {
        viewport,
        wgpu_options,
        ..Default::default()
    };
    eframe::run_native(
        "plotx",
        native_options,
        Box::new(move |cc| {
            #[cfg(windows)]
            apply_windows_frame_polish(cc);
            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            cc.egui_ctx.set_fonts(fonts);
            // The UI-scale commands own Ctrl+= / Ctrl+- / Ctrl+0 so the change
            // persists per monitor; egui's built-in handler would apply an
            // unrecorded zoom on top.
            cc.egui_ctx
                .options_mut(|options| options.zoom_with_keyboard = false);
            ui::apply_chrome_theme(&cc.egui_ctx, settings.appearance.theme);
            let updated = plotx_core::update::launched_after_update(&settings.app_version);
            let mut app = PlotxApp::new_with_settings(settings);
            let mut recovery = match plotx_core::project::RecoveryManager::new() {
                Ok(recovery) => Some(recovery),
                Err(error) => {
                    app.session.status = format!("Could not initialize recovery storage: {error}");
                    None
                }
            };
            let pending_recovery = match recovery.as_mut() {
                Some(recovery) => match recovery.pending_recovery() {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        app.session.status = format!("Could not inspect recovery data: {error}");
                        None
                    }
                },
                None => None,
            };
            let mut pending_crash_report = observability::pending_crash_report();
            let crash_notice = if pending_recovery.is_none() {
                pending_crash_report.take().map(|path| {
                    observability::acknowledge_crash_report();
                    format!(
                        "PlotX did not shut down cleanly last time. A crash report was saved to {}.",
                        path.display()
                    )
                })
            } else {
                None
            };
            if updated {
                app.session.status = format!("Updated to PlotX {}.", env!("CARGO_PKG_VERSION"));
                // Stamp the new version so the notice shows only once.
                plotx_core::settings::update(|_| {});
            }
            if let Some(notice) = crash_notice {
                if updated {
                    app.session.status.push(' ');
                    app.session.status.push_str(&notice);
                } else {
                    app.session.status = notice;
                }
            }
            #[cfg(target_os = "macos")]
            let native_menu =
                ui::native_menu::NativeMenu::new(&app, &cc.egui_ctx).map_err(|error| {
                    std::io::Error::other(format!("failed to install macOS menu: {error}"))
                })?;
            Ok(Box::new(Shell {
                app,
                recovery,
                pending_recovery,
                pending_crash_report,
                recovery_job: None,
                recovery_written: false,
                next_recovery_at: Instant::now() + RECOVERY_INTERVAL,
                clipboard_table_paste: Default::default(),
                batch_workflow: Default::default(),
                shot: shot::ShotDriver::from_env(),
                // The screenshot harness scripts its own zoom; adaptive scale
                // must not fight it.
                scale: scale::ScaleDriver::new(!shot_active),
                #[cfg(target_os = "macos")]
                native_menu,
            }))
        }),
    )?;
    if let Some(error) = SHOT_FAILURE.lock().unwrap().take() {
        log::error!("screenshot harness failed: {error}");
        log::logger().flush();
        std::process::exit(1);
    }
    if let Some(plan) = PENDING_INSTALL.lock().unwrap().take()
        && let Err(error) = plan.launch(RELAUNCH_REQUESTED.load(Ordering::Relaxed))
    {
        log::error!("failed to launch update helper: {error}");
    }
    log::logger().flush();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canceling_close_clears_update_restart_intent() {
        RELAUNCH_REQUESTED.store(false, Ordering::Relaxed);
        request_relaunch();
        assert!(RELAUNCH_REQUESTED.load(Ordering::Relaxed));
        cancel_relaunch();
        assert!(!RELAUNCH_REQUESTED.load(Ordering::Relaxed));
    }
}
