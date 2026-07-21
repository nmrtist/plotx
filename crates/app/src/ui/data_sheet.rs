use super::*;

mod cell;
mod column_menu;
mod grid;
mod relationship;
mod transform;
mod typed;

pub(super) fn data_sheet_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(di) = app.session.ui.sheet_open else {
        return;
    };
    if di >= app.doc.datasets.len() {
        app.session.ui.sheet_open = None;
        return;
    }
    let mut open = true;
    let mut commit: Option<plotx_core::state::TableEditDelta> = None;
    let mut transform = None;
    let mut refresh = None;
    let catalog = app
        .doc
        .datasets
        .iter()
        .enumerate()
        .filter_map(|(dataset, value)| {
            let table = value.as_table()?;
            let revision = &table.typed_state.envelope.revision;
            Some(transform::TableCatalogEntry {
                dataset,
                name: value.display_name(),
                read: plotx_core::data::SnapshotRead {
                    table: revision.table_id,
                    revision: revision.id,
                    fingerprint: revision.snapshot.fingerprint,
                },
                schema: revision.snapshot.schema.clone(),
            })
        })
        .collect::<Vec<_>>();
    let title = format!("Data sheet — {}", app.doc.datasets[di].display_name());
    egui::Window::new(title)
        .collapsible(false)
        .resizable(true)
        .default_size([620.0, 380.0])
        .open(&mut open)
        .show(ctx, |ui| {
            if let Some(elapsed) = app.table_transform_progress() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!(
                        "Running table transform… {:.1} s",
                        elapsed.as_secs_f32()
                    ));
                    if ui.button("Cancel").clicked() {
                        app.cancel_table_transform();
                    }
                });
                ui.separator();
            }
            if let Some(n) = app.doc.datasets[di].as_nmr() {
                nmr_sheet(ui, n);
            } else if let Some(n) = app.doc.datasets[di].as_nmr2d() {
                nmr2d_sheet(ui, n);
            } else if app.doc.datasets[di].as_table().is_some() {
                let t = app.doc.datasets[di].as_table_mut().unwrap();
                typed::typed_table_sheet(
                    ui,
                    t,
                    transform::TableSheetContext {
                        dataset: di,
                        commit: &mut commit,
                        transform: &mut transform,
                        refresh: &mut refresh,
                        catalog: &catalog,
                        transform_running: app.session.table_transform_job.is_some()
                            || app.session.table_refresh_job.is_some(),
                    },
                );
            }
        });
    if let Some(delta) = commit {
        let typed_diagnostic = delta.typed_diagnostic.clone();
        app.execute_action(Action::edit_table(di, delta));
        app.session.status = typed_diagnostic.unwrap_or_else(|| "Edited data table.".to_owned());
    }
    if let Some(request) = transform
        && let Err(error) = app.start_table_transform(
            request.plan,
            request.input_datasets,
            request.name,
            256 * 1024 * 1024,
        )
    {
        app.session.status = error;
    }
    if let Some((dataset, inputs)) = refresh
        && let Err(error) = app.start_table_refresh(dataset, inputs, 256 * 1024 * 1024)
    {
        app.session.status = error;
    }
    if !open {
        app.session.ui.sheet_open = None;
    }
}

pub(super) fn nmr2d_sheet(ui: &mut Ui, n: &plotx_core::state::Nmr2DDataset) {
    let d = &n.data;
    ui.label(format!(
        "{} × {} points · indirect quadrature {:?}",
        d.cols, d.rows, d.quad
    ));
    ui.label(format!(
        "Direct (F2): {} · {:.3} MHz · SW {:.0} Hz · carrier {:.2} ppm",
        d.direct.nucleus,
        d.direct.observe_freq_mhz,
        d.direct.spectral_width_hz,
        d.direct.carrier_ppm
    ));
    ui.label(format!(
        "Indirect (F1): {} · {:.3} MHz · SW {:.0} Hz · carrier {:.2} ppm",
        d.indirect.nucleus,
        d.indirect.observe_freq_mhz,
        d.indirect.spectral_width_hz,
        d.indirect.carrier_ppm
    ));
    if let Some(exp) = &d.experiment {
        ui.label(format!("Experiment hint: {exp}"));
    }
    ui.separator();
    match &n.processed {
        plotx_processing::Processed2D::Ft(s) => {
            let (f2lo, f2hi) = s.f2_bounds();
            let (f1lo, f1hi) = s.f1_bounds();
            ui.label(format!(
                "Contour spectrum {}×{} (F1×F2) — F2 {f2lo:.2}..{f2hi:.2} ppm, F1 {f1lo:.2}..{f1hi:.2} ppm",
                s.f1_size, s.f2_size
            ));
        }
        plotx_processing::Processed2D::Stack(s) => {
            ui.label(format!(
                "Pseudo-2D stack of {} direct-dimension spectra",
                s.increments()
            ));
        }
    }
}

pub(super) fn nmr_sheet(ui: &mut Ui, n: &plotx_core::state::NmrDataset) {
    let spec = &n.spectrum;
    let len = spec.len();
    ui.label(format!(
        "{} · {} pts · {:.3} MHz · {:.3} Hz/pt",
        spec.nucleus, len, spec.observe_freq_mhz, spec.hz_per_point
    ));
    ui.separator();

    let columns: Vec<(String, Vec<f64>)> = vec![
        ("ppm".to_owned(), spec.ppm.clone()),
        ("Real".to_owned(), spec.real()),
        (
            "Imag".to_owned(),
            spec.values.iter().map(|c| c.im).collect(),
        ),
        ("Magnitude".to_owned(), spec.magnitude()),
    ];

    egui::ScrollArea::horizontal().show(ui, |ui| {
        ui.horizontal_top(|ui| {
            for (header, values) in &columns {
                ui.vertical(|ui| {
                    ui.set_width(140.0);
                    ui.strong(header);
                    ui.weak(format!("{len} values (collapsed)"));
                    collapsed_column(ui, values);
                });
            }
        });
    });
}

// Each numeric column has tens of thousands of values, so it is shown collapsed:
// a greyed cell with a sparkline of the values, not the raw rows.
pub(super) fn collapsed_column(ui: &mut Ui, values: &[f64]) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(132.0, 240.0), Sense::hover());
    let painter = ui.painter();
    let grey = if ui.visuals().dark_mode {
        Color32::from_white_alpha(14)
    } else {
        Color32::from_black_alpha(12)
    };
    painter.rect_filled(rect, 3.0, grey);
    sparkline(painter, rect.shrink(8.0), values);
}

pub(super) fn sparkline(painter: &egui::Painter, rect: egui::Rect, values: &[f64]) {
    if values.len() < 2 {
        return;
    }
    let target = 160usize;
    let stride = (values.len() / target).max(1);
    let sampled: Vec<f64> = values.iter().step_by(stride).copied().collect();
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in &sampled {
        if v.is_finite() {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    if !lo.is_finite() || (hi - lo).abs() < f64::EPSILON {
        let y = rect.center().y;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0_f32, Color32::GRAY),
        );
        return;
    }
    let span = hi - lo;
    let n = sampled.len();
    let pts: Vec<Pos2> = sampled
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let tx = i as f32 / (n - 1) as f32;
            let ty = ((v - lo) / span) as f32;
            Pos2::new(
                rect.left() + tx * rect.width(),
                rect.bottom() - ty * rect.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        pts,
        Stroke::new(1.0_f32, Color32::from_rgb(0x1f, 0x6f, 0xeb)),
    ));
}
