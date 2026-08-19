use super::*;
use plotx_figure::{ErrorBar, Series};

struct PreparedLine {
    index: usize,
    x_bounds: [f64; 2],
    series: Vec<Series>,
    error_bars: Vec<ErrorBar>,
}

impl PlotxApp {
    /// Whether a binding has one stackable representation. Item-addressed line
    /// traces use their field contracts and may cross enclosing data domains;
    /// ordinary fields retain the legacy same-domain rules.
    pub fn series_stackable(&self, binding: &DataBinding) -> bool {
        let Some(domain) = binding
            .series
            .first()
            .and_then(|s| self.doc.dataset_index(s.source.resource))
            .and_then(|index| self.doc.datasets.get(index))
            .map(Dataset::domain)
        else {
            return false;
        };
        let same_domain = binding.series.iter().all(|s| {
            self.doc
                .dataset_index(s.source.resource)
                .and_then(|index| self.doc.datasets.get(index))
                .map(Dataset::domain)
                == Some(domain)
        });
        let item_addressed = binding
            .series
            .iter()
            .all(|series| series.source.item.is_some());
        let all_lines = binding
            .series
            .iter()
            .all(|series| matches!(series.encoding, plotx_figure::SeriesEncoding::Line(_)));
        let all_contours = binding
            .series
            .iter()
            .all(|series| matches!(series.encoding, plotx_figure::SeriesEncoding::Contour(_)));
        if item_addressed {
            all_lines
        } else {
            same_domain
                && ((all_lines && domain.stack_kind() == Some(StackKind::Line)) || all_contours)
        }
    }

    /// Combine a stackable binding into one figure. Concrete encodings decide
    /// whether this is a contour overlay or a line stack; the enclosing domain
    /// may expose both kinds of field.
    pub fn build_stacked_figure(
        &mut self,
        binding: &DataBinding,
        stack: &StackSpec,
        size_mm: [f32; 2],
    ) -> Figure {
        let contour_overlay = binding
            .series
            .iter()
            .all(|series| matches!(series.encoding, plotx_figure::SeriesEncoding::Contour(_)));
        if contour_overlay {
            self.build_contour_overlay(binding, size_mm)
        } else {
            self.build_line_stack(binding, stack, size_mm)
        }
    }

    /// Line-kind stacking: each series is built generically through the chart
    /// registry (its domain's line chart); `stack` controls per-trace scale,
    /// visibility, normalization and vertical/horizontal offset.
    fn build_line_stack(
        &mut self,
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
        let primary_is_encoded_curve = binding
            .series
            .first()
            .is_some_and(|series| self.series_uses_encoded_curve(series));
        let mut fig = if primary_is_encoded_curve {
            binding
                .series
                .first()
                .and_then(|series| self.build_encoded_series_figure(series))
                .map(|figure| self.normalize_binding_figure(figure, size_mm))
                .unwrap_or_else(|| self.build_full_canvas_figure(primary, &line_chart, size_mm))
        } else {
            self.build_full_canvas_figure(primary, &line_chart, size_mm)
        };
        if let Some(primary_binding) = binding.series.first() {
            self.apply_series_binding_style(&mut fig, primary_binding);
        }
        let x_span = (fig.x.max - fig.x.min).abs().max(f64::MIN_POSITIVE);
        fig.series.clear();
        fig.error_bars.clear();

        // Build each visible trace's (scaled, optionally normalized) line series,
        // tracking the global peak the vertical offset scales against.
        let mut prepared = Vec::new();
        let mut global_peak = 0.0f64;
        for (i, sb) in binding.series.iter().enumerate() {
            let Some(dataset) = self.doc.dataset_index(sb.source.resource) else {
                continue;
            };
            if !sb.visible {
                continue;
            }
            let encoded_curve = self.series_uses_encoded_curve(sb);
            let mut part = if encoded_curve {
                self.build_encoded_series_figure(sb)
                    .map(|figure| self.normalize_binding_figure(figure, size_mm))
                    .unwrap_or_else(|| self.build_full_canvas_figure(dataset, &line_chart, size_mm))
            } else {
                self.build_full_canvas_figure(dataset, &line_chart, size_mm)
            };
            self.apply_series_binding_style(&mut part, sb);
            let part_x_bounds = [part.x.min, part.x.max];
            let mut series = part.series;
            let mut error_bars = part.error_bars;
            let peak = series
                .iter()
                .flat_map(|s| s.points.iter())
                .fold(0.0f64, |m, p| m.max(p[1].abs()));
            let factor = if stack.normalize && peak > 0.0 {
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
            prepared.push(PreparedLine {
                index: i,
                x_bounds: part_x_bounds,
                series,
                error_bars,
            });
        }

        let stacked = matches!(stack.mode, StackMode::Offset);
        let (mut x_min, mut x_max) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut y_min, mut y_max) = (fig.y.min, fig.y.max);
        for prepared in prepared {
            let i = prepared.index;
            let part_x_bounds = prepared.x_bounds;
            let mut series = prepared.series;
            let mut error_bars = prepared.error_bars;
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
            x_min = x_min.min(part_x_bounds[0] + x_off);
            x_max = x_max.max(part_x_bounds[1] + x_off);
            for mut s in series.drain(..) {
                for p in &mut s.points {
                    p[0] += x_off;
                    p[1] += y_off;
                    x_min = x_min.min(p[0]);
                    x_max = x_max.max(p[0]);
                    y_min = y_min.min(p[1]);
                    y_max = y_max.max(p[1]);
                }
                if active {
                    s.width = s.width.max(1.0) * 2.0;
                }
                fig.series.push(s);
            }
            for mut error_bar in error_bars.drain(..) {
                error_bar.center[0] += x_off;
                error_bar.center[1] += y_off;
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
        if x_min.is_finite() && x_max.is_finite() {
            fig.x.min = x_min;
            fig.x.max = x_max;
        }
        fig.y.min = y_min;
        fig.y.max = y_max;
        if !binding.primary_visible() {
            fig.integral_curves.clear();
        }
        fig
    }

    /// Field-kind stacking (`ColorOverlay`): overlay every selected 2D dataset's
    /// contour using its persisted per-series style, merging the datasets' x/y
    /// ranges. The primary supplies the axis labels and orientation; hidden
    /// series are skipped.
    fn build_contour_overlay(&mut self, binding: &DataBinding, size_mm: [f32; 2]) -> Figure {
        let chart = ChartSpec::default_for(DataDomain::Nmr2d);
        let primary = binding
            .primary_dataset()
            .and_then(|id| self.doc.dataset_index(id))
            .expect("validated data binding has a primary dataset");
        let primary_part = binding
            .series
            .first()
            .and_then(|series| self.build_encoded_series_figure(series))
            .unwrap_or_else(|| self.build_full_canvas_figure(primary, &chart, size_mm));
        let mut fig = primary_part.clone();
        fig.contours.clear();
        let (mut x_min, mut x_max) = (fig.x.min, fig.x.max);
        let (mut y_min, mut y_max) = (fig.y.min, fig.y.max);
        let mut merged = false;
        for (i, sb) in binding.series.iter().enumerate() {
            if !sb.visible {
                continue;
            }
            let encoded_contour = matches!(sb.encoding, plotx_figure::SeriesEncoding::Contour(_));
            let part = if i == 0 {
                primary_part.clone()
            } else if let Some(part) = self.build_encoded_series_figure(sb) {
                part
            } else {
                continue;
            };
            let color = sb
                .primary_color()
                .unwrap_or(OVERLAY_PALETTE[i % OVERLAY_PALETTE.len()]);
            for mut contour in part.contours {
                // A concrete ContourSpec owns independent positive and negative
                // styling. The legacy chart fallback retains its palette override
                // until the broad stack migration lands in phase 7.
                if !encoded_contour {
                    contour.color = color;
                }
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
        self.normalize_binding_figure(fig, size_mm)
    }

    pub fn series_label(&self, sb: &SeriesBinding) -> String {
        sb.label.clone().unwrap_or_else(|| {
            self.doc
                .dataset_by_id(sb.source.resource)
                .map(|dataset| {
                    sb.source
                        .item
                        .and_then(|item| dataset.trace_item_label(sb.source.field, item))
                        .unwrap_or_else(|| dataset.display_name())
                })
                .unwrap_or_default()
        })
    }

    /// Dataset indices eligible to stack onto `binding`: other datasets of the
    /// same stackable domain not already bound. Item-addressed trace collections
    /// can be line-stacked across datasets even when their enclosing domain also
    /// exposes non-line fields.
    pub fn stack_candidates(&self, binding: &DataBinding) -> Vec<usize> {
        let Some(domain) = binding
            .primary_dataset()
            .and_then(|id| self.doc.dataset_by_id(id))
            .map(Dataset::domain)
        else {
            return Vec::new();
        };
        if domain.stack_kind().is_none()
            && !binding
                .series
                .iter()
                .all(|series| series.source.item.is_some())
        {
            return Vec::new();
        }
        let bound = binding.dataset_ids();
        let item_addressed = binding
            .series
            .iter()
            .all(|series| series.source.item.is_some());
        (0..self.doc.datasets.len())
            .filter(|di| {
                self.doc.datasets.get(*di).map(Dataset::domain) == Some(domain)
                    && !bound.contains(&self.doc.datasets[*di].resource_id())
                    && (!item_addressed
                        || trace_collection_field(&self.doc.datasets[*di]).is_some())
                    && (item_addressed
                        || binding.series.first().is_some_and(|source| {
                            default_field_encoding_matches(
                                &self.doc.datasets[*di],
                                &source.encoding,
                            )
                        }))
            })
            .collect()
    }

    /// Materialize every source compatible with the binding being extended.
    /// Item-addressed plots expose the whole trace collection even when that
    /// dataset's own live display currently selects a scalar map.
    pub fn stack_candidate_series_options(
        &self,
        binding: &DataBinding,
        dataset: usize,
    ) -> Vec<SeriesBinding> {
        let Some(dataset) = self.doc.datasets.get(dataset) else {
            return Vec::new();
        };
        if binding
            .series
            .iter()
            .all(|series| series.source.item.is_some())
        {
            let Some(field) = trace_collection_field(dataset) else {
                return Vec::new();
            };
            return SeriesBinding::from_field_all(dataset, field);
        }
        SeriesBinding::from_dataset_all(dataset)
    }

    /// Materialize the first compatible source for non-interactive callers.
    pub fn stack_candidate_series(
        &self,
        binding: &DataBinding,
        dataset: usize,
    ) -> Option<SeriesBinding> {
        self.stack_candidate_series_options(binding, dataset)
            .into_iter()
            .next()
    }

    /// Labels and stable IDs available when retargeting one item-addressed
    /// series. The plot keeps its own ID and styling when its source item changes.
    pub fn series_item_options(
        &self,
        series: &SeriesBinding,
    ) -> Vec<(plotx_data::TraceItemId, String)> {
        let Some(collection) = self
            .doc
            .dataset_by_id(series.source.resource)
            .and_then(|dataset| dataset.trace_collection(series.source.field))
        else {
            return Vec::new();
        };
        collection
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let label = item
                    .automatic_label()
                    .unwrap_or_else(|| format!("{} {}", collection.axis_quantity, index + 1));
                (item.id, label)
            })
            .collect()
    }

    /// Choose a default for a newly inserted series without changing any
    /// existing authored colors. Prefer an unused palette entry; cycling is the
    /// unavoidable fallback only after the plot already uses the full palette.
    pub fn next_stack_color(&self, binding: &DataBinding) -> plotx_figure::Color {
        OVERLAY_PALETTE
            .iter()
            .copied()
            .find(|color| {
                binding
                    .series
                    .iter()
                    .all(|series| series.primary_color() != Some(*color))
            })
            .unwrap_or(OVERLAY_PALETTE[binding.series.len() % OVERLAY_PALETTE.len()])
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

    /// The selected datasets in the Data list if they form a valid stack (at
    /// least two in one domain), in selection order. Trace collections derive
    /// applicability from their concrete fields rather than the domain's
    /// legacy stack kind.
    pub fn stackable_selection(&self) -> Option<Vec<usize>> {
        let sel = &self.session.ui.data_selection;
        if sel.len() < 2 {
            return None;
        }
        let trace_count = sel
            .iter()
            .filter_map(|index| self.doc.datasets.get(*index))
            .filter(|dataset| dataset.active_trace_collection_field().is_some())
            .count();
        if trace_count > 0 {
            (trace_count == sel.len() && self.trace_selection_compatible(sel)).then(|| sel.clone())
        } else {
            let domain = self.doc.datasets.get(*sel.first()?).map(Dataset::domain)?;
            (domain.stack_kind().is_some()
                && sel
                    .iter()
                    .all(|&d| self.doc.datasets.get(d).map(Dataset::domain) == Some(domain)))
            .then(|| sel.clone())
        }
    }

    /// Build a new page whose single plot stacks the currently multi-selected
    /// datasets, as one undoable step. Item-addressed sources first open the
    /// trace composer; non-trace stacks retain the immediate command behavior.
    pub fn stack_selected_data(&mut self) {
        let Some(sel) = self.stackable_selection() else {
            return;
        };
        let trace_source_count = sel
            .iter()
            .filter_map(|index| self.doc.datasets.get(*index))
            .filter(|dataset| dataset.active_trace_collection_field().is_some())
            .count();
        if trace_source_count > 0 {
            if trace_source_count == sel.len()
                && let Some(composer) = self.trace_composer_for_selection(&sel)
            {
                let item_count = composer.items.len();
                self.session.ui.trace_composer = Some(composer);
                self.session.status = format!(
                    "Choose traces for the new stack. {item_count} items selected by default."
                );
            } else {
                self.session.status =
                    "The selected trace collections do not have compatible axes and units."
                        .to_owned();
            }
            return;
        }
        let series = sel
            .iter()
            .filter_map(|&d| self.doc.datasets.get(d))
            .flat_map(SeriesBinding::from_dataset_all)
            .collect::<Vec<_>>();
        self.insert_stack_canvas(&sel, series, false);
    }

    pub(super) fn insert_stack_canvas(
        &mut self,
        selection: &[usize],
        mut series: Vec<SeriesBinding>,
        trace_items: bool,
    ) -> bool {
        let Some(&primary) = selection.first() else {
            return false;
        };
        let domain = self.doc.datasets[primary].domain();
        for (index, series) in series.iter_mut().enumerate() {
            series.set_primary_color(OVERLAY_PALETTE[index % OVERLAY_PALETTE.len()]);
        }
        let binding = DataBinding { series };
        let series_count = binding.series.len();
        let mode = if trace_items {
            if !binding
                .series
                .iter()
                .all(|series| matches!(series.encoding, plotx_figure::SeriesEncoding::Line(_)))
            {
                return false;
            }
            StackMode::Offset
        } else {
            match domain.stack_kind() {
                Some(StackKind::Field) => StackMode::ColorOverlay,
                _ => StackMode::Offset,
            }
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
        let mut plot = PlotObject::new(
            None,
            SeriesId::new(0),
            binding,
            chart,
            stack,
            AxisProjections::default(),
            AxisOverrides::default(),
            figure,
            viewport,
        );
        // One place decides how a freshly materialized binding is numbered, so
        // the ids and the allocator cannot drift apart.
        plot.mint_series_ids();
        canvas.objects.push(CanvasObject {
            id,
            name: self.default_plot_title(primary),
            frame,
            locked: false,
            visible: true,
            kind: CanvasObjectKind::Plot(Box::new(plot)),
        });
        canvas
            .create_panel_for_plot(id)
            .expect("a newly materialized stack object is a plot");
        let index = self.doc.canvases.len();
        let canvas_count = self.doc.canvases.len();
        self.execute_action(Action::insert_canvas(
            index,
            canvas,
            self.session.active_canvas,
        ));
        if self.doc.canvases.len() != canvas_count + 1 {
            return false;
        }
        if trace_items {
            self.reveal_board_frame(FrameRef::Page(index));
            self.session.ui.selection_scope = SelectionScope::CanvasObjects;
            self.session.ui.selection_anchors = SelectionAnchors::default();
            self.set_selection(Selection::single(id));
            self.session.ui.requested_inspector_section = Some("inspector.data".to_owned());
        }
        self.clear_selection();
        self.session.status = if trace_items {
            let trace_word = if series_count == 1 { "trace" } else { "traces" };
            let dataset_word = if selection.len() == 1 {
                "dataset"
            } else {
                "datasets"
            };
            format!(
                "Stacked {series_count} {trace_word} from {} {dataset_word} on a new page.",
                selection.len(),
            )
        } else {
            format!("Stacked {} datasets on a new page.", selection.len())
        };
        true
    }
}

fn trace_collection_field(dataset: &Dataset) -> Option<FieldId> {
    dataset.active_trace_collection_field()
}

fn default_field_encoding_matches(
    dataset: &Dataset,
    encoding: &plotx_figure::SeriesEncoding,
) -> bool {
    let recommended = dataset
        .default_field_id()
        .and_then(|field| dataset.field_descriptor(field))
        .and_then(|field| field.metadata.recommended_encoding().map(str::to_owned));
    matches!(
        (encoding, recommended.as_deref()),
        (plotx_figure::SeriesEncoding::Line(_), Some("line"))
            | (plotx_figure::SeriesEncoding::Contour(_), Some("contour"))
            | (plotx_figure::SeriesEncoding::Heatmap(_), Some("heatmap"))
            | (plotx_figure::SeriesEncoding::Image(_), Some("image"))
    )
}
