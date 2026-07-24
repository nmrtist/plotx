//! Screenshot harness: set `PLOTX_SHOT` to an output directory and the app
//! drives a scripted synthetic session, writes PNG frame captures there, and
//! exits. Development aid only; inert without the env var.
//!
//! The session is a flat list of [`Scene`]s (see [`SCENES`]): each scene waits a
//! fixed number of frames for pending compute and layout to settle, then runs an
//! optional mutation and/or captures a named screenshot. To add or reorder a
//! capture, edit that list — no frame arithmetic is threaded across the file.
//!
//! Every scene is captured once per palette by replaying the whole list in the
//! light theme and again in the dark theme, so a UX audit gets matching pairs
//! without a second invocation. Set `PLOTX_SHOT_THEME` to `light` or `dark` to
//! restrict the run to a single palette. Captures land at
//! `<PLOTX_SHOT>/<theme>/<scene>.png`.
//!
//! The checked-in scene list is deliberately minimal: a canonical fit workflow
//! and the Ribbon width-budget breakpoints, states that are worth re-checking
//! on any UI change. For a task-specific audit, extend the list locally and
//! drop the additions once the task is done.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::UserData;
use num_complex::Complex64;
use plotx_core::actions::Action;
use plotx_core::settings::Settings;
use plotx_core::state::{
    AnalysisSelection, AxisRange, DEFAULT_CANVAS_SIZE_MM, Dataset, FrameRef, LineShapeKind,
    NmrDataset, PlotxApp, Tool,
};
use plotx_io::{Domain, NmrData};

const FIT_LO: f64 = 1.4;
const FIT_HI: f64 = 3.2;

/// Generous ceiling for the whole run (two palette passes). Real runs finish in
/// a few seconds; this only guards a wedged session so CI fails loudly.
const SHOT_TIMEOUT: Duration = Duration::from_secs(120);

/// Window size every pass starts from, so the light and dark capture of a scene
/// share dimensions instead of inheriting whatever the previous pass left.
const BASE_WINDOW: [f32; 2] = [1500.0, 1000.0];

/// A palette to replay the full scene list under.
#[derive(Clone, Copy)]
enum Theme {
    Light,
    Dark,
}

impl Theme {
    fn preference(self) -> egui::ThemePreference {
        match self {
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
        }
    }

    /// Output subdirectory name for this palette.
    fn label(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }
}

/// A mutation applied at the start of a scene, before its screenshot. Parameters
/// live in the variant so the scene list stays a plain data table.
#[derive(Clone, Copy)]
enum Op {
    /// Reset zoom and window size, then load the synthetic FID and arm a fit.
    Setup,
    /// Fit the FID band and frame the result.
    LineFit,
    Zoom(f32),
    Resize(f32, f32),
}

/// One scripted beat: advance `settle` frames so pending compute and layout
/// quiesce, then apply `op` (if any) and capture `shot` (if any) on that frame.
#[derive(Clone, Copy)]
struct Scene {
    settle: u32,
    op: Option<Op>,
    shot: Option<&'static str>,
}

const fn act(settle: u32, op: Op) -> Scene {
    Scene {
        settle,
        op: Some(op),
        shot: None,
    }
}

const fn shot(settle: u32, name: &'static str) -> Scene {
    Scene {
        settle,
        op: None,
        shot: Some(name),
    }
}

/// The scripted session, replayed once per palette. Settle counts are the frame
/// gaps the previous frame-number state machine encoded implicitly; the larger
/// gaps wait out asynchronous fits.
///
/// A zoom change and a window resize must never share a frame: eframe converts a
/// requested inner size to physical pixels through the current zoom, and issuing
/// both together races so the capture size depends on the pass's prior zoom. Set
/// zoom in its own scene and let it settle before the next resize.
const SCENES: &[Scene] = &[
    act(2, Op::Zoom(0.75)),
    act(2, Op::Setup),
    shot(8, "band"),
    act(2, Op::LineFit),
    shot(10, "fitted"),
    // The three widths bracket the Ribbon's width budget: 720 forces the
    // dense layout with the "More" overflow, 900 sits between the density
    // breakpoints, and 1440 shows every group expanded. The Ribbon lays out
    // by measured width, so these are the sizes where regressions appear.
    act(2, Op::Zoom(1.0)),
    act(2, Op::Resize(720.0, 700.0)),
    shot(10, "ribbon_720"),
    act(2, Op::Resize(900.0, 760.0)),
    shot(12, "ribbon_900"),
    act(2, Op::Resize(1440.0, 900.0)),
    shot(12, "ribbon_1440"),
];

pub struct ShotDriver {
    dir: PathBuf,
    /// Palettes still to replay; one full pass over `SCENES` each.
    themes: Vec<Theme>,
    theme_idx: usize,
    scene_idx: usize,
    /// Frames left before the current scene fires.
    wait: u32,
    /// Whether the current pass's fresh document and theme have been applied.
    pass_primed: bool,
    saved: usize,
    /// Total captures across every palette pass.
    expected: usize,
    started_at: Instant,
    failed: bool,
}

impl ShotDriver {
    pub fn from_env() -> Option<Self> {
        let dir = std::env::var_os("PLOTX_SHOT")?;
        let themes = themes_from_env();
        let per_pass = SCENES.iter().filter(|s| s.shot.is_some()).count();
        Some(Self {
            dir: dir.into(),
            expected: per_pass * themes.len(),
            themes,
            theme_idx: 0,
            scene_idx: 0,
            wait: 0,
            pass_primed: false,
            saved: 0,
            started_at: Instant::now(),
            failed: false,
        })
    }

    pub fn drive(&mut self, app: &mut PlotxApp, ctx: &egui::Context) {
        if self.failed {
            request_exit(app, ctx);
            return;
        }

        // Screenshots are delivered asynchronously a frame or two after the
        // request, so drain and save whatever has arrived every frame.
        self.collect(app, ctx);
        if self.failed {
            return;
        }
        if self.saved >= self.expected {
            request_exit(app, ctx);
            return;
        }
        if self.started_at.elapsed() >= SHOT_TIMEOUT {
            self.fail(
                app,
                ctx,
                format!(
                    "timed out after {}s with {}/{} screenshots saved",
                    SHOT_TIMEOUT.as_secs(),
                    self.saved,
                    self.expected
                ),
            );
            return;
        }

        self.step(app, ctx);
        if self.failed {
            return;
        }
        ctx.request_repaint();
    }

    /// Advance the scripted state machine, issuing screenshot requests. Exit is
    /// gated separately on `saved >= expected` so in-flight captures still land.
    fn step(&mut self, app: &mut PlotxApp, ctx: &egui::Context) {
        if self.theme_idx >= self.themes.len() {
            // Every pass has been issued; idle while the last captures drain.
            return;
        }
        if !self.pass_primed {
            // Replay from an identical clean slate so a scene's light and dark
            // captures differ only in palette, never in accumulated state.
            *app = PlotxApp::new_with_settings(Settings::default());
            ctx.set_theme(self.themes[self.theme_idx].preference());
            self.scene_idx = 0;
            self.wait = SCENES[0].settle;
            self.pass_primed = true;
            return; // let the fresh document and theme take effect next frame
        }
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }

        let scene = SCENES[self.scene_idx];
        if let Some(op) = scene.op
            && let Err(error) = run_op(op, app, ctx)
        {
            self.fail(app, ctx, error);
            return;
        }
        if let Some(name) = scene.shot {
            let theme = self.themes[self.theme_idx].label();
            shoot(ctx, format!("{theme}/{name}"));
        }

        self.scene_idx += 1;
        if self.scene_idx >= SCENES.len() {
            self.theme_idx += 1;
            self.pass_primed = false;
        } else {
            self.wait = SCENES[self.scene_idx].settle;
        }
    }

    fn collect(&mut self, app: &mut PlotxApp, ctx: &egui::Context) {
        let shots: Vec<(String, Arc<egui::ColorImage>)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot {
                        image, user_data, ..
                    } => {
                        let name = user_data.data.as_ref()?.downcast_ref::<String>()?.clone();
                        Some((name, image.clone()))
                    }
                    _ => None,
                })
                .collect()
        });
        for (rel, image) in shots {
            if let Err(error) = save_png(&self.dir.join(format!("{rel}.png")), &image) {
                self.fail(app, ctx, format!("failed to save {rel}: {error}"));
                return;
            }
            self.saved += 1;
        }
    }

    fn fail(&mut self, app: &mut PlotxApp, ctx: &egui::Context, error: String) {
        self.failed = true;
        crate::record_shot_failure(error);
        request_exit(app, ctx);
    }
}

/// Palettes to capture: both by default, or the single one named by
/// `PLOTX_SHOT_THEME`.
fn themes_from_env() -> Vec<Theme> {
    match std::env::var("PLOTX_SHOT_THEME").ok().as_deref() {
        Some("light") => vec![Theme::Light],
        Some("dark") => vec![Theme::Dark],
        _ => vec![Theme::Light, Theme::Dark],
    }
}

fn run_op(op: Op, app: &mut PlotxApp, ctx: &egui::Context) -> Result<(), String> {
    match op {
        Op::Setup => {
            // Resize explicitly instead of inheriting the previous pass's window;
            // zoom is already settled from the preceding scene.
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                BASE_WINDOW[0],
                BASE_WINDOW[1],
            )));
            setup(app);
        }
        Op::LineFit => line_fit(app, ctx)?,
        Op::Zoom(factor) => ctx.set_zoom_factor(factor),
        Op::Resize(w, h) => {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
        }
    }
    Ok(())
}

fn request_exit(app: &mut PlotxApp, ctx: &egui::Context) {
    // Synthetic sessions are deliberately dirty. They never represent user work,
    // so the harness must bypass the production Save / Discard / Cancel prompt.
    app.session.allow_close = true;
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
}

fn shoot(ctx: &egui::Context, name: String) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(UserData::new(name)));
}

fn setup(app: &mut PlotxApp) {
    let data = synthetic_fid();
    let action = Action::insert_dataset_with_default_canvas(
        app,
        Dataset::Nmr(Box::new(NmrDataset::load(data))),
        "Canvas 1 — synthetic".to_owned(),
        DEFAULT_CANVAS_SIZE_MM,
    );
    app.execute_action(action);
    app.set_tool(Tool::LineFit);
    if let Some(ci) = app.session.active_canvas
        && let Some(object) = app.doc.canvases[ci].active_plot_object_id()
    {
        app.session.ui.analysis_selection = Some(AnalysisSelection {
            dataset: app.doc.datasets[0].resource_id(),
            canvas: app.doc.canvases[ci].resource_id,
            object,
            x_range: AxisRange::new(FIT_LO, FIT_HI),
            y_range: None,
        });
    }
}

fn line_fit(app: &mut PlotxApp, ctx: &egui::Context) -> Result<(), String> {
    app.run_line_fit(0, FIT_LO, FIT_HI, LineShapeKind::Lorentzian)
        .map_err(|error| format!("line fit failed: {error}"))?;
    app.session.ui.analysis_selection = None;
    app.session.active_canvas = Some(0);
    if let Some(id) = app.doc.canvases[0].active_plot_object_id() {
        app.select_object(0, id);
    }
    app.focus_single(0);
    crate::ui::canvas::request_board_fit(app, ctx, FrameRef::Page(0));
    Ok(())
}

fn synthetic_fid() -> NmrData {
    let npoints = 4096;
    let (sw, obs, carrier) = (4000.0, 400.0, 5.0);
    let dt = 1.0 / sw;
    let peaks = [(1.8, 1.0, 0.15), (2.3, 0.6, 0.10), (2.6, 0.8, 0.20)];
    let points = (0..npoints)
        .map(|k| {
            let t = k as f64 * dt;
            peaks
                .iter()
                .map(|&(ppm, amp, t2): &(f64, f64, f64)| {
                    let freq_hz = (ppm - carrier) * obs;
                    Complex64::from_polar(
                        amp * (-t / t2).exp(),
                        std::f64::consts::TAU * freq_hz * t,
                    )
                })
                .sum()
        })
        .collect();
    NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz: sw,
        observe_freq_mhz: obs,
        carrier_ppm: carrier,
        nucleus: "1H".to_owned(),
        source: "synthetic".to_owned(),
        group_delay: 0.0,
    }
}

fn save_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    let [width, height] = image.size;
    // egui screenshots are opaque RGBA8, so straight-alpha encoding is exact.
    image::save_buffer_with_format(
        path,
        image.as_raw(),
        width as u32,
        height as u32,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| format!("encode {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automated_exit_bypasses_dirty_project_prompt() {
        let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
        app.doc.dirty = true;
        let ctx = egui::Context::default();

        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            request_exit(&mut app, ui.ctx());
        });

        assert!(app.session.allow_close);
        let root = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("root viewport output");
        assert!(root.commands.contains(&egui::ViewportCommand::Close));
    }

    #[test]
    fn expected_count_covers_every_scene_in_both_palettes() {
        let per_pass = SCENES.iter().filter(|s| s.shot.is_some()).count();
        assert_eq!(per_pass, 5, "scene list should define 5 captures");
        // Default run (no PLOTX_SHOT_THEME) replays every scene in both palettes.
        assert_eq!(per_pass * 2, 10);
    }

    #[test]
    fn const_builders_tag_scenes_consistently() {
        assert!(act(2, Op::Setup).shot.is_none());
        assert!(shot(4, "band").op.is_none());
        assert_eq!(shot(4, "band").shot, Some("band"));
    }
}
