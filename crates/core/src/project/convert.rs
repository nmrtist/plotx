use super::convert_recipes::{nmr2d_recipe_extensions, read_regions};
use super::electrophysiology_convert::{
    electrophysiology_from_object, electrophysiology_to_objects,
};
use super::*;

pub fn dataset_to_objects(
    dataset: &Dataset,
    data_id: &str,
    recipe_id: &str,
) -> Result<(DataObject, Vec<u8>, RecipeObject)> {
    Ok(match dataset {
        Dataset::Nmr(n) => {
            let blob = complex_to_bytes(&n.data.points);
            let data = DataObject {
                id: data_id.to_owned(),
                role: "data".to_owned(),
                classification: nmr_acquisition_classification(),
                label: n.name.clone(),
                dimensions: vec![dimension_from_1d(&n.data)],
                payload: Payload {
                    storage: STORAGE_COMPLEX_F64_LE.to_owned(),
                    blob: format!("objects/{data_id}/data.bin"),
                    shape: vec![n.data.points.len()],
                    domain: domain_to_str(n.data.domain).to_owned(),
                },
                extensions: serde_json::json!({
                    "plotx.nmr": {
                        "source": &n.data.source
                    }
                }),
            };
            let recipe = RecipeObject {
                id: recipe_id.to_owned(),
                role: "recipe".to_owned(),
                classification: nmr_recipe_classification(),
                input: data_id.to_owned(),
                parameters: RecipeParameters {
                    dimension_count: 1,
                    pipelines: vec![pipeline_to_dto(&n.pipeline)],
                    group_delay_correct: n.group_delay_correct,
                    ..RecipeParameters::default()
                },
                extensions: serde_json::json!({
                    "plotx.analysis": {
                        "peaks": &n.peaks,
                        "integrals": &n.integrals,
                        "line_fits": &n.line_fits,
                        "multiplets": &n.multiplets
                    }
                }),
            };
            (data, blob, recipe)
        }
        Dataset::Nmr2D(n) => {
            let blob = complex_to_bytes(&n.data.data);
            let data = DataObject {
                id: data_id.to_owned(),
                role: "data".to_owned(),
                classification: nmr_acquisition_classification(),
                label: n.name.clone(),
                dimensions: vec![
                    dimension_from_dim("f1", "indirect", 0, n.data.rows, &n.data.indirect),
                    dimension_from_dim("f2", "direct", 1, n.data.cols, &n.data.direct),
                ],
                payload: Payload {
                    storage: STORAGE_COMPLEX_F64_LE.to_owned(),
                    blob: format!("objects/{data_id}/data.bin"),
                    shape: vec![n.data.rows, n.data.cols],
                    domain: domain_to_str(n.data.domain).to_owned(),
                },
                extensions: serde_json::json!({
                    "plotx.nmr": {
                        "source": &n.data.source,
                        "quad": quad_to_str(n.data.quad),
                        "indirect_conjugate": n.data.indirect_conjugate,
                        "experiment_hint": &n.data.experiment,
                        "pseudo_axis": n.data.pseudo_axis.as_ref().map(pseudo_axis_to_dto),
                        "diffusion": n.data.diffusion.as_ref().map(diffusion_to_dto),
                    }
                }),
            };
            let recipe = RecipeObject {
                id: recipe_id.to_owned(),
                role: "recipe".to_owned(),
                classification: nmr_recipe_classification(),
                input: data_id.to_owned(),
                parameters: RecipeParameters {
                    dimension_count: 2,
                    pipelines: vec![pipeline_to_dto(&n.params.f2), pipeline_to_dto(&n.params.f1)],
                    group_delay_correct: n.group_delay_correct,
                    layout: Some(layout_to_str(n.params.layout).to_owned()),
                    preset: Some(preset_to_str(n.preset).to_owned()),
                },
                extensions: nmr2d_recipe_extensions(n),
            };
            (data, blob, recipe)
        }
        Dataset::Table(t) => {
            return Err(ProjectError::Invalid(format!(
                "typed table {} reached the generic object encoder",
                t.resource_id
            )));
        }
        Dataset::Electrophysiology(recording) => {
            electrophysiology_to_objects(recording, data_id, recipe_id)?
        }
    })
}

pub fn object_to_dataset(
    zip: &mut zip::ZipArchive<File>,
    data: &DataObject,
    recipe: &RecipeObject,
) -> Result<Dataset> {
    if data.classification.domain == "electrophysiology"
        && data.classification.object == "recording"
    {
        return electrophysiology_from_object(zip, data);
    }
    if data.classification.object == "table" {
        if data.payload.storage != STORAGE_TABLE_V1 {
            return Err(ProjectError::Unsupported(
                "legacy DataTable payload; this project must be regenerated".to_owned(),
            ));
        }
        return table_dataset_from_v1(zip, data).map(|table| Dataset::Table(Box::new(table)));
    }
    if data.classification.domain != "spectroscopy"
        || data.classification.technique.as_deref() != Some("nmr")
        || data.classification.object != "acquisition"
    {
        return Err(ProjectError::Unsupported(format!(
            "data classification {}/{:?}/{}",
            data.classification.domain, data.classification.technique, data.classification.object
        )));
    }
    if data.payload.storage != STORAGE_COMPLEX_F64_LE {
        return Err(ProjectError::Unsupported(format!(
            "payload storage {}",
            data.payload.storage
        )));
    }
    let raw = read_bytes(zip, &data.payload.blob)?;
    let values = complex_from_bytes(&raw)?;
    match data.dimensions.len() {
        1 => {
            let dim = data.dimensions.first().unwrap();
            let expected = data.payload.shape.first().copied().unwrap_or(dim.size);
            if values.len() != expected {
                return Err(ProjectError::Invalid(format!(
                    "1D data length {} does not match shape {expected}",
                    values.len()
                )));
            }
            let mut dataset = NmrDataset::load(NmrData {
                points: values,
                domain: domain_from_str(&data.payload.domain),
                spectral_width_hz: required(dim.spectral_width_hz, "spectral_width_hz")?,
                observe_freq_mhz: required(dim.observe_freq_mhz, "observe_freq_mhz")?,
                carrier_ppm: required(dim.carrier_ppm, "carrier_ppm")?,
                nucleus: dim.nucleus.clone().unwrap_or_else(|| "X".to_owned()),
                source: nmr_source(data),
                group_delay: dim.group_delay.unwrap_or(0.0),
            });
            apply_1d_recipe(&mut dataset, recipe);
            dataset.name = data.label.clone();
            dataset.retransform();
            Ok(Dataset::Nmr(Box::new(dataset)))
        }
        2 => {
            let rows = *data
                .payload
                .shape
                .first()
                .ok_or_else(|| ProjectError::Invalid("2D payload missing rows".to_owned()))?;
            let cols = *data
                .payload
                .shape
                .get(1)
                .ok_or_else(|| ProjectError::Invalid("2D payload missing cols".to_owned()))?;
            if values.len() != rows * cols {
                return Err(ProjectError::Invalid(format!(
                    "2D data length {} does not match shape {}x{}",
                    values.len(),
                    rows,
                    cols
                )));
            }
            let direct = data
                .dimensions
                .iter()
                .find(|d| d.role == "direct")
                .or_else(|| data.dimensions.iter().find(|d| d.storage_axis == 1))
                .ok_or_else(|| {
                    ProjectError::Invalid("2D data missing direct dimension".to_owned())
                })?;
            let indirect = data
                .dimensions
                .iter()
                .find(|d| d.role == "indirect")
                .or_else(|| data.dimensions.iter().find(|d| d.storage_axis == 0))
                .ok_or_else(|| {
                    ProjectError::Invalid("2D data missing indirect dimension".to_owned())
                })?;
            let mut dataset = Nmr2DDataset::load(NmrData2D {
                data: values,
                rows,
                cols,
                domain: domain_from_str(&data.payload.domain),
                direct: dim_from_dimension(direct)?,
                indirect: dim_from_dimension(indirect)?,
                quad: quad_from_str(nmr_ext_str(data, "quad").unwrap_or("complex")),
                indirect_conjugate: nmr_ext_bool(data, "indirect_conjugate").unwrap_or(false),
                experiment: nmr_ext_str(data, "experiment_hint").map(str::to_owned),
                pseudo_axis: read_pseudo_axis(data),
                diffusion: read_diffusion(data),
                nus: None,
                source: nmr_source(data),
            });
            apply_2d_recipe(&mut dataset, recipe);
            read_regions(&mut dataset, recipe);
            read_integrals_2d(&mut dataset, recipe)?;
            dataset.name = data.label.clone();
            dataset.retransform();
            if let Err(error) = dataset.recompute_integrals() {
                dataset.integral_error = Some(error.to_string());
            }
            Ok(Dataset::Nmr2D(Box::new(dataset)))
        }
        n => Err(ProjectError::Unsupported(format!(
            "NMR acquisitions with {n} dimensions"
        ))),
    }
}

use crate::state::{AxisProjection, AxisProjections, ProjectionSource};

fn projections_to_dto(p: &AxisProjections, datasets: &[Dataset]) -> Result<Option<ProjectionsDto>> {
    if p.is_empty() {
        return Ok(None);
    }
    Ok(Some(ProjectionsDto {
        top: axis_projection_to_dto(&p.top, datasets)?,
        left: axis_projection_to_dto(&p.left, datasets)?,
    }))
}

fn axis_projection_to_dto(
    a: &AxisProjection,
    datasets: &[Dataset],
) -> Result<Option<AxisProjectionDto>> {
    let (source, attached, slice_index) = match a.source {
        ProjectionSource::None => return Ok(None),
        ProjectionSource::Attached(d) => (
            "attached",
            Some(format!(
                "recipe_{}",
                datasets
                    .get(d)
                    .ok_or_else(|| {
                        ProjectError::Invalid(format!(
                            "axis projection references missing dataset {d}"
                        ))
                    })?
                    .resource_id()
            )),
            0,
        ),
        ProjectionSource::Sum => ("sum", None, 0),
        ProjectionSource::Skyline => ("skyline", None, 0),
        ProjectionSource::Slice(i) => ("slice", None, i),
    };
    Ok(Some(AxisProjectionDto {
        source: source.to_owned(),
        attached,
        slice_index,
        visible: a.visible,
    }))
}

fn projections_from_dto(
    dto: &ProjectionsDto,
    recipe_to_dataset: &HashMap<String, usize>,
) -> AxisProjections {
    AxisProjections {
        top: axis_projection_from_dto(dto.top.as_ref(), recipe_to_dataset),
        left: axis_projection_from_dto(dto.left.as_ref(), recipe_to_dataset),
    }
}

fn axis_projection_from_dto(
    dto: Option<&AxisProjectionDto>,
    recipe_to_dataset: &HashMap<String, usize>,
) -> AxisProjection {
    let Some(dto) = dto else {
        return AxisProjection::default();
    };
    let source = match dto.source.as_str() {
        "attached" => dto
            .attached
            .as_ref()
            .and_then(|id| recipe_to_dataset.get(id).copied())
            .map(ProjectionSource::Attached)
            .unwrap_or(ProjectionSource::None),
        "sum" => ProjectionSource::Sum,
        "skyline" => ProjectionSource::Skyline,
        "slice" => ProjectionSource::Slice(dto.slice_index),
        _ => ProjectionSource::None,
    };
    AxisProjection {
        source,
        visible: dto.visible,
    }
}

pub fn canvas_to_view(
    datasets: &[Dataset],
    canvas: &CanvasDocument,
    view_id: &str,
) -> Result<ViewObject> {
    let objects: Vec<ViewCanvasObject> = canvas
        .objects
        .iter()
        .map(|object| {
            let base = |kind: &str| ViewCanvasObject {
                id: object.id.to_string(),
                name: object.name.clone(),
                kind: kind.to_owned(),
                input: String::new(),
                series: Vec::new(),
                chart_type: None,
                chart_column: None,
                chart_bins: None,
                chart_stacked: false,
                chart_colormap: None,
                chart_view: None,
                stack: None,
                projections: None,
                frame: FrameDto::from_frame(object.frame),
                viewport: None,
                panel: None,
                title: None,
                text: None,
                shape: None,
                locked: object.locked,
                visible: object.visible,
                group: object.group,
                snapshot: None,
            };
            match &object.kind {
                CanvasObjectKind::Plot(plot) => {
                    let primary = plot.primary_dataset();
                    let primary_dataset = datasets.get(primary).ok_or_else(|| {
                        ProjectError::Invalid(format!(
                            "view {view_id} plot {} references missing primary dataset {primary}",
                            object.id
                        ))
                    })?;
                    let kind = match primary_dataset {
                        Dataset::Nmr(_) => "line_plot",
                        Dataset::Nmr2D(n) => match n.params.layout {
                            Layout2D::Ft => "contour_plot",
                            Layout2D::Stack => "stack_plot",
                        },
                        Dataset::Table(_) => "line_plot",
                        Dataset::Electrophysiology(_) => "line_plot",
                    };
                    let series = plot
                        .binding
                        .series
                        .iter()
                        .map(|sb| {
                            let dataset = datasets.get(sb.dataset).ok_or_else(|| {
                                ProjectError::Invalid(format!(
                                    "view {view_id} plot {} references missing series dataset {}",
                                    object.id, sb.dataset
                                ))
                            })?;
                            Ok(SeriesBindingDto {
                                input: format!("recipe_{}", dataset.resource_id()),
                                color: sb.color.map(|c| [c.r, c.g, c.b]),
                                label: sb.label.clone(),
                                scale: sb.scale,
                                visible: sb.visible,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let stack = (plot.stack != StackSpec::default())
                        .then(|| StackDto::from_spec(&plot.stack));
                    Ok(ViewCanvasObject {
                        input: format!("recipe_{}", primary_dataset.resource_id()),
                        series,
                        chart_type: Some(plot.chart.type_id.clone()),
                        chart_column: plot.chart.column.map(|column| column.to_string()),
                        chart_bins: plot.chart.bins,
                        chart_stacked: plot.chart.stacked,
                        chart_colormap: (plot.chart.colormap
                            != plotx_figure::ColormapId::default())
                        .then(|| plot.chart.colormap.id().to_owned()),
                        chart_view: (plot.chart.view_angles != crate::state::SURFACE_DEFAULT_VIEW)
                            .then_some(plot.chart.view_angles),
                        stack,
                        projections: projections_to_dto(&plot.projections, datasets)?,
                        viewport: Some(ViewportDto::from_viewport(&plot.viewport)),
                        panel: Some(PanelDto::from_panel(&plot.panel)),
                        ..base(kind)
                    })
                }
                CanvasObjectKind::Text(t) => Ok(ViewCanvasObject {
                    text: Some(TextBoxDto::from_text_box(t)),
                    ..base("text")
                }),
                CanvasObjectKind::PanelLabel(t) => Ok(ViewCanvasObject {
                    text: Some(TextBoxDto::from_text_box(t)),
                    ..base("panel_label")
                }),
                CanvasObjectKind::Shape(s) => Ok(ViewCanvasObject {
                    shape: Some(ShapeDto::from_shape(s)),
                    ..base("shape")
                }),
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ViewObject {
        id: view_id.to_owned(),
        role: "view".to_owned(),
        classification: Classification {
            domain: "visualization".to_owned(),
            technique: Some("spectral_plot".to_owned()),
            object: "page".to_owned(),
        },
        inputs: objects
            .iter()
            .map(|object| object.input.clone())
            .filter(|input| !input.is_empty())
            .collect(),
        name: canvas.name.clone(),
        caption: canvas.caption.clone(),
        caption_visible: canvas.caption_visible,
        panel_label_style: Some(canvas.panel_label_style.as_key().to_owned()),
        layout: ViewLayout {
            size_mm: canvas.size_mm,
            size_preset: canvas.size_preset_id.clone(),
            auto_height: canvas.auto_height,
            grid: Some(PageLayoutDto::from_layout(&canvas.layout)),
            background: Some([
                canvas.background.r,
                canvas.background.g,
                canvas.background.b,
            ]),
            board_pos: Some(canvas.board_pos),
        },
        objects,
        viewport: None,
        snapshot: None,
    })
}

pub fn view_to_canvas(
    app: &mut PlotxApp,
    zip: &mut zip::ZipArchive<File>,
    view_id: &str,
    view: &ViewObject,
    index: usize,
    recipe_to_dataset: &HashMap<String, usize>,
) -> Result<CanvasDocument> {
    let mut canvas = CanvasDocument::new(view.name.clone(), view.layout.size_mm);
    canvas.size_preset_id = view.layout.size_preset.clone();
    canvas.auto_height = view.layout.auto_height;
    canvas.board_pos = view
        .layout
        .board_pos
        .unwrap_or_else(|| crate::state::default_board_layout(index));
    canvas.caption = view.caption.clone();
    canvas.caption_visible = view.caption_visible;
    canvas.panel_label_style = view
        .panel_label_style
        .as_deref()
        .map(crate::state::PanelLabelStyle::from_key)
        .unwrap_or_default();
    canvas.layout = view
        .layout
        .grid
        .map(PageLayoutDto::into_layout)
        .unwrap_or_default();
    if let Some([r, g, b]) = view.layout.background {
        canvas.background = plotx_figure::Color::rgb(r, g, b);
    }
    let mut max_id = 0;
    let mut max_group = 0;
    for view_object in &view.objects {
        let object_id = view_object
            .id
            .parse::<u64>()
            .map_err(|_| ProjectError::Invalid(format!("invalid object id {}", view_object.id)))?;
        let frame = view_object.frame.into_frame();
        let kind = match view_object.kind.as_str() {
            "text" => CanvasObjectKind::Text(text_box_from(view_object, false)),
            "panel_label" => CanvasObjectKind::PanelLabel(text_box_from(view_object, true)),
            "shape" => CanvasObjectKind::Shape(
                view_object
                    .shape
                    .clone()
                    .map(ShapeDto::into_shape)
                    .unwrap_or_else(|| ShapeObject::new(ShapeKind::Rect)),
            ),
            "line_plot" | "contour_plot" | "stack_plot" | "plot" => {
                let resolve = |input: &str| {
                    recipe_to_dataset.get(input).copied().ok_or_else(|| {
                        ProjectError::Invalid(format!(
                            "view {view_id} references unknown recipe {input}"
                        ))
                    })
                };
                let binding = if view_object.series.is_empty() {
                    DataBinding::single(resolve(&view_object.input)?)
                } else {
                    let mut series = Vec::with_capacity(view_object.series.len());
                    for sb in &view_object.series {
                        series.push(SeriesBinding {
                            dataset: resolve(&sb.input)?,
                            color: sb.color.map(|c| plotx_figure::Color::rgb(c[0], c[1], c[2])),
                            label: sb.label.clone(),
                            scale: sb.scale,
                            visible: sb.visible,
                        });
                    }
                    DataBinding { series }
                };
                let stack = view_object
                    .stack
                    .clone()
                    .map(StackDto::into_spec)
                    .unwrap_or_default();
                let di = binding.primary_dataset();
                let domain = app
                    .doc
                    .datasets
                    .get(di)
                    .map(Dataset::domain)
                    .unwrap_or(crate::state::DataDomain::Nmr1d);
                let chart = crate::state::ChartSpec {
                    type_id: view_object
                        .chart_type
                        .clone()
                        .unwrap_or_else(|| crate::state::default_chart_type(domain).id.to_owned()),
                    column: view_object
                        .chart_column
                        .as_deref()
                        .map(str::parse::<plotx_data::ColumnId>)
                        .transpose()
                        .map_err(|error| {
                            ProjectError::Invalid(format!(
                                "view {} has an invalid chart column id: {error}",
                                view.id
                            ))
                        })?,
                    bins: view_object.chart_bins,
                    stacked: view_object.chart_stacked,
                    // Unknown ids (from a newer build) fall back to the default map.
                    colormap: view_object
                        .chart_colormap
                        .as_deref()
                        .and_then(plotx_figure::ColormapId::from_id)
                        .unwrap_or_default(),
                    view_angles: view_object
                        .chart_view
                        .unwrap_or(crate::state::SURFACE_DEFAULT_VIEW),
                };
                let size_mm = [
                    frame.width / crate::state::MM_TO_PT,
                    frame.height / crate::state::MM_TO_PT,
                ];
                let projections = view_object
                    .projections
                    .as_ref()
                    .map(|dto| projections_from_dto(dto, recipe_to_dataset))
                    .unwrap_or_default();
                let mut figure = if let Some(snapshot) = &view_object.snapshot {
                    read_json(zip, &snapshot.figure).unwrap_or_else(|_| {
                        app.build_object_figure(&binding, &chart, &stack, &projections, size_mm)
                    })
                } else {
                    app.build_object_figure(&binding, &chart, &stack, &projections, size_mm)
                };
                let viewport = view_object
                    .viewport
                    .as_ref()
                    .map(ViewportDto::to_viewport)
                    .unwrap_or_else(|| CanvasViewport::from_figure(&figure));
                if view_object.snapshot.is_none() {
                    viewport.apply_to(&mut figure);
                }
                figure.title.clear();
                let panel = view_object
                    .panel
                    .clone()
                    .or_else(|| view_object.title.clone())
                    .map(PanelDto::into_panel)
                    .unwrap_or_else(|| PanelMeta::new(app.default_plot_title(di), frame.width));
                CanvasObjectKind::Plot(Box::new(PlotObject {
                    binding,
                    chart,
                    stack,
                    projections,
                    figure,
                    viewport,
                    panel,
                }))
            }
            _ => continue,
        };
        canvas.objects.push(CanvasObject {
            id: object_id,
            name: view_object.name.clone(),
            frame,
            locked: view_object.locked,
            visible: view_object.visible,
            group: view_object.group,
            kind,
        });
        max_id = max_id.max(object_id);
        max_group = max_group.max(view_object.group.unwrap_or(0));
    }
    canvas.next_object_id = max_id + 1;
    canvas.next_group_id = max_group + 1;
    Ok(canvas)
}

fn text_box_from(view_object: &ViewCanvasObject, panel: bool) -> TextBox {
    view_object
        .text
        .clone()
        .map(TextBoxDto::into_text_box)
        .unwrap_or_else(|| {
            if panel {
                TextBox::panel_label(String::new())
            } else {
                TextBox::label(String::new())
            }
        })
}

pub fn dimension_from_1d(data: &NmrData) -> Dimension {
    Dimension {
        id: "f2".to_owned(),
        role: "direct".to_owned(),
        size: data.points.len(),
        storage_axis: 0,
        quantity: "time_or_frequency".to_owned(),
        display_quantity: Some("chemical_shift".to_owned()),
        unit: Some("ppm".to_owned()),
        nucleus: Some(data.nucleus.clone()),
        spectral_width_hz: Some(data.spectral_width_hz),
        observe_freq_mhz: Some(data.observe_freq_mhz),
        carrier_ppm: Some(data.carrier_ppm),
        group_delay: Some(data.group_delay),
    }
}

pub fn dimension_from_dim(
    id: &str,
    role: &str,
    storage_axis: usize,
    size: usize,
    dim: &Dim,
) -> Dimension {
    Dimension {
        id: id.to_owned(),
        role: role.to_owned(),
        size,
        storage_axis,
        quantity: "time_or_frequency".to_owned(),
        display_quantity: Some("chemical_shift".to_owned()),
        unit: Some("ppm".to_owned()),
        nucleus: Some(dim.nucleus.clone()),
        spectral_width_hz: Some(dim.spectral_width_hz),
        observe_freq_mhz: Some(dim.observe_freq_mhz),
        carrier_ppm: Some(dim.carrier_ppm),
        group_delay: Some(dim.group_delay),
    }
}

pub fn dim_from_dimension(dim: &Dimension) -> Result<Dim> {
    Ok(Dim {
        spectral_width_hz: required(dim.spectral_width_hz, "spectral_width_hz")?,
        observe_freq_mhz: required(dim.observe_freq_mhz, "observe_freq_mhz")?,
        carrier_ppm: required(dim.carrier_ppm, "carrier_ppm")?,
        nucleus: dim.nucleus.clone().unwrap_or_else(|| "X".to_owned()),
        group_delay: dim.group_delay.unwrap_or(0.0),
    })
}
