use super::*;
use plotx_figure::{ErrorBar, Series};

impl PlotxApp {
    /// Whether every dataset in `binding` shares one stackable domain (hence one
    /// [`StackKind`]), so they can be combined into a single overlay/stack figure.
    pub fn series_stackable(&self, binding: &DataBinding) -> bool {
        let Some(domain) = binding
            .series
            .first()
            .and_then(|s| self.doc.dataset_index(s.dataset))
            .and_then(|index| self.doc.datasets.get(index))
            .map(Dataset::domain)
        else {
            return false;
        };
        domain.stack_kind().is_some()
            && binding.series.iter().all(|s| {
                self.doc
                    .dataset_index(s.dataset)
                    .and_then(|index| self.doc.datasets.get(index))
                    .map(Dataset::domain)
                    == Some(domain)
            })
    }

    /// Combine a stackable binding into one figure. Dispatches on the primary's
    /// [`StackKind`] and the stack `mode`: Line kinds overlay/offset traces; the
    /// Field kind overlays each dataset's 2D contour in a distinct colour.
    pub fn build_stacked_figure(
        &self,
        binding: &DataBinding,
        stack: &StackSpec,
        size_mm: [f32; 2],
    ) -> Figure {
        let primary = binding
            .primary_dataset()
            .and_then(|id| self.doc.dataset_index(id))
            .expect("validated data binding has a primary dataset");
        let domain = self.doc.datasets[primary].domain();
        match (domain.stack_kind(), stack.mode) {
            (Some(StackKind::Field), _) => self.build_contour_overlay(binding, size_mm),
            _ => self.build_line_stack(binding, stack, size_mm),
        }
    }

    /// Line-kind stacking: each series is built generically through the chart
    /// registry (its domain's line chart); `stack` controls per-trace scale,
    /// visibility, normalization and vertical/horizontal offset.
    fn build_line_stack(
        &self,
        binding: &DataBinding,
        stack: &StackSpec,
        size_mm: [f32; 2],
    ) -> Figure {
        let primary = binding
            .primary_dataset()
            .and_then(|id| self.doc.dataset_index(id))
            .expect("validated data binding has a primary dataset");
        let domain = self.doc.datasets[primary].domain();
        let line_chart = ChartSpec::default_for(domain);
        // The primary's line figure supplies the axis labels and orientation.
        let mut fig = self.build_full_canvas_figure(primary, &line_chart, size_mm);
        let x_span = (fig.x.max - fig.x.min).abs().max(f64::MIN_POSITIVE);
        fig.series.clear();
        fig.error_bars.clear();

        // Build each visible trace's (scaled, optionally normalized) line series,
        // tracking the global peak the vertical offset scales against.
        let mut prepared: Vec<(usize, Vec<Series>, Vec<ErrorBar>)> = Vec::new();
        let mut global_peak = 0.0f64;
        for (i, sb) in binding.series.iter().enumerate() {
            let Some(dataset) = self.doc.dataset_index(sb.dataset) else {
                continue;
            };
            if !sb.visible {
                continue;
            }
            let part = self.build_full_canvas_figure(dataset, &line_chart, size_mm);
            let mut series = part.series;
            let mut error_bars = part.error_bars;
            let peak = series
                .iter()
                .flat_map(|s| s.points.iter())
                .fold(0.0f64, |m, p| m.max(p[1].abs()));
            let factor = sb.scale
                * if stack.normalize && peak > 0.0 {
                    1.0 / peak
                } else {
                    1.0
                };
            let mut trace_peak = 0.0f64;
            for s in &mut series {
                for p in &mut s.points {
                    p[1] *= factor;
                    trace_peak = trace_peak.max(p[1].abs());
                }
            }
            for error_bar in &mut error_bars {
                error_bar.center[1] *= factor;
                error_bar.negative *= factor.abs();
                error_bar.positive *= factor.abs();
            }
            global_peak = global_peak.max(trace_peak);
            prepared.push((i, series, error_bars));
        }

        let stacked = matches!(stack.mode, StackMode::Offset);
        let (mut x_min, mut x_max) = (fig.x.min, fig.x.max);
        let (mut y_min, mut y_max) = (fig.y.min, fig.y.max);
        for (i, mut series, mut error_bars) in prepared {
            let sb = &binding.series[i];
            let color = sb
                .color
                .unwrap_or(OVERLAY_PALETTE[i % OVERLAY_PALETTE.len()]);
            let label = self.series_label(sb);
            let x_off = if stacked {
                i as f64 * stack.shear_x * x_span
            } else {
                0.0
            };
            let y_off = if stacked {
                i as f64 * stack.spacing_y * global_peak
            } else {
                0.0
            };
            let active = stack.active == Some(i);
            for mut s in series.drain(..) {
                for p in &mut s.points {
                    p[0] += x_off;
                    p[1] += y_off;
                    x_min = x_min.min(p[0]);
                    x_max = x_max.max(p[0]);
                    y_min = y_min.min(p[1]);
                    y_max = y_max.max(p[1]);
                }
                s.color = color;
                s.name = label.clone();
                if active {
                    s.width = s.width.max(1.0) * 2.0;
                }
                fig.series.push(s);
            }
            for mut error_bar in error_bars.drain(..) {
                error_bar.center[0] += x_off;
                error_bar.center[1] += y_off;
                error_bar.color = color;
                if active {
                    error_bar.width = error_bar.width.max(1.0) * 2.0;
                }
                x_min = x_min.min(error_bar.center[0]);
                x_max = x_max.max(error_bar.center[0]);
                y_min = y_min.min(error_bar.center[1] - error_bar.negative);
                y_max = y_max.max(error_bar.center[1] + error_bar.positive);
                fig.error_bars.push(error_bar);
            }
        }
        fig.x.min = x_min;
        fig.x.max = x_max;
        fig.y.min = y_min;
        fig.y.max = y_max;
        if !binding.primary_visible() {
            fig.integral_curves.clear();
        }
        fig.show_legend = true;
        fig
    }

    /// Field-kind stacking (`ColorOverlay`): overlay every selected 2D dataset's
    /// contour on one canvas, each recoloured from the palette (or its per-series
    /// override), merging the datasets' x/y ranges. The primary supplies the axis
    /// labels and orientation; hidden series are skipped.
    fn build_contour_overlay(&self, binding: &DataBinding, size_mm: [f32; 2]) -> Figure {
        let chart = ChartSpec::default_for(DataDomain::Nmr2d);
        let primary = binding
            .primary_dataset()
            .and_then(|id| self.doc.dataset_index(id))
            .expect("validated data binding has a primary dataset");
        let mut fig = self.build_full_canvas_figure(primary, &chart, size_mm);
        fig.contours.clear();
        let (mut x_min, mut x_max) = (fig.x.min, fig.x.max);
        let (mut y_min, mut y_max) = (fig.y.min, fig.y.max);
        let mut merged = false;
        for (i, sb) in binding.series.iter().enumerate() {
            let Some(dataset) = self.doc.dataset_index(sb.dataset) else {
                continue;
            };
            if !sb.visible {
                continue;
            }
            let part = self.build_full_canvas_figure(dataset, &chart, size_mm);
            let color = sb
                .color
                .unwrap_or(OVERLAY_PALETTE[i % OVERLAY_PALETTE.len()]);
            for mut contour in part.contours {
                contour.color = color;
                fig.contours.push(contour);
            }
            if merged {
                x_min = x_min.min(part.x.min);
                x_max = x_max.max(part.x.max);
                y_min = y_min.min(part.y.min);
                y_max = y_max.max(part.y.max);
            } else {
                (x_min, x_max, y_min, y_max) = (part.x.min, part.x.max, part.y.min, part.y.max);
                merged = true;
            }
        }
        fig.x.min = x_min;
        fig.x.max = x_max;
        fig.y.min = y_min;
        fig.y.max = y_max;
        fig.show_legend = true;
        fig
    }

    pub fn series_label(&self, sb: &SeriesBinding) -> String {
        sb.label.clone().unwrap_or_else(|| {
            self.doc
                .dataset_by_id(sb.dataset)
                .map(Dataset::display_name)
                .unwrap_or_default()
        })
    }

    /// Dataset indices eligible to stack onto `binding`: other datasets of the
    /// same stackable domain not already bound. Empty when the plot's primary is
    /// not a stackable domain.
    pub fn stack_candidates(&self, binding: &DataBinding) -> Vec<usize> {
        let Some(domain) = binding
            .primary_dataset()
            .and_then(|id| self.doc.dataset_by_id(id))
            .map(Dataset::domain)
            .filter(|d| d.stack_kind().is_some())
        else {
            return Vec::new();
        };
        let bound = binding.dataset_ids();
        (0..self.doc.datasets.len())
            .filter(|di| {
                self.doc.datasets.get(*di).map(Dataset::domain) == Some(domain)
                    && !bound.contains(&self.doc.datasets[*di].resource_id())
            })
            .collect()
    }

    /// The focused dataset: the lead (last) element of the selection set. Drives the
    /// object inspector, secondary-sidebar tools, analysis, breadcrumb and shortcuts.
    /// The single source of truth — a stored active dataset can no longer disagree
    /// with the multi-select that the Stack command counts.
    pub fn active_dataset(&self) -> Option<usize> {
        self.session.ui.data_selection.last().copied()
    }

    /// Make `di` the sole selection (a plain Data-list click), so it also leads.
    pub fn focus_single(&mut self, di: usize) {
        self.session.ui.data_selection = vec![di];
    }

    /// Replace the selection with `items`; when `lead` is given it is moved to the
    /// end so it becomes the active dataset, otherwise `items`' last element leads.
    pub fn focus_datasets(&mut self, items: &[usize], lead: Option<usize>) {
        match lead {
            Some(lead) => {
                let mut set: Vec<usize> = items.iter().copied().filter(|&d| d != lead).collect();
                set.push(lead);
                self.session.ui.data_selection = set;
            }
            None => self.session.ui.data_selection = items.to_vec(),
        }
    }

    /// Add `di` to the selection as its new lead (moving it to the end if already
    /// present).
    pub fn add_to_selection(&mut self, di: usize) {
        if let Some(pos) = self.session.ui.data_selection.iter().position(|&d| d == di) {
            self.session.ui.data_selection.remove(pos);
        }
        self.session.ui.data_selection.push(di);
    }

    /// Focus a single dataset, or clear the selection when `None` — for sites that
    /// derive the active dataset from a canvas/object that may have none.
    pub fn set_active_dataset(&mut self, active: Option<usize>) {
        match active {
            Some(di) => self.focus_single(di),
            None => self.clear_selection(),
        }
    }

    pub fn clear_selection(&mut self) {
        self.session.ui.data_selection.clear();
    }

    /// Apply a Data-list click to the selection model. `extend` (Shift/Ctrl) toggles
    /// `di` in the multi-select; a plain click makes `di` the sole selection. The
    /// active dataset follows for free: adding pushes `di` to the lead position, and
    /// removing the lead promotes the previous item, `None` only when the set empties.
    pub fn toggle_selection(&mut self, di: usize, extend: bool) {
        if !extend {
            self.focus_single(di);
        } else if let Some(pos) = self.session.ui.data_selection.iter().position(|&d| d == di) {
            self.session.ui.data_selection.remove(pos);
        } else {
            self.add_to_selection(di);
        }
    }

    /// The selected datasets in the Data list if they form a valid stack (≥2 and
    /// sharing one stackable domain), in selection order. Drives the "Stack
    /// selected data" command's enablement.
    pub fn stackable_selection(&self) -> Option<Vec<usize>> {
        let sel = &self.session.ui.data_selection;
        if sel.len() < 2 {
            return None;
        }
        let domain = self
            .doc
            .datasets
            .get(*sel.first()?)
            .map(Dataset::domain)
            .filter(|d| d.stack_kind().is_some())?;
        sel.iter()
            .all(|&d| self.doc.datasets.get(d).map(Dataset::domain) == Some(domain))
            .then(|| sel.clone())
    }

    /// Build a new page whose single plot stacks the currently multi-selected
    /// datasets, as one undoable step. No-op unless the selection is a valid stack.
    pub fn stack_selected_data(&mut self) {
        let Some(sel) = self.stackable_selection() else {
            return;
        };
        let domain = self.doc.datasets[sel[0]].domain();
        let binding = DataBinding {
            series: sel
                .iter()
                .filter_map(|&d| self.doc.datasets.get(d))
                .map(|dataset| SeriesBinding::new(dataset.resource_id()))
                .collect(),
        };
        let mode = match domain.stack_kind() {
            Some(StackKind::Field) => StackMode::ColorOverlay,
            _ => StackMode::Offset,
        };
        let stack = StackSpec {
            mode,
            ..StackSpec::default()
        };
        let chart = ChartSpec::default_for(domain);
        let name = format!("Canvas {} — Stack", self.doc.canvases.len() + 1);
        let mut canvas = CanvasDocument::new(name, DEFAULT_CANVAS_SIZE_MM);
        let page = canvas.size_pt();
        let id = canvas.allocate_object_id();
        let frame = ObjectFrame::new(0.0, 0.0, page[0], page[1]);
        let figure = self.build_binding_figure(&binding, &chart, &stack, canvas.size_mm);
        let viewport = CanvasViewport::from_figure(&figure);
        let panel = PanelMeta::new(self.default_plot_title(sel[0]), frame.width);
        canvas.objects.push(CanvasObject {
            id,
            name: "Plot 1".to_owned(),
            frame,
            locked: false,
            visible: true,
            group: None,
            kind: CanvasObjectKind::Plot(Box::new(PlotObject {
                binding,
                chart,
                stack,
                projections: AxisProjections::default(),
                axis_overrides: AxisOverrides::default(),
                figure,
                viewport,
                panel,
            })),
        });
        let index = self.doc.canvases.len();
        self.execute_action(Action::insert_canvas(
            index,
            canvas,
            self.session.active_canvas,
        ));
        self.clear_selection();
        self.session.status = format!("Stacked {} datasets on a new page.", sel.len());
    }
}
