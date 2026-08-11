use super::axis_overrides::AxisOverridesDto;
use super::field_catalog::validate_series_source;
use super::*;
use crate::state::SeriesId;
use crate::state::{AssetId, ImageFit, ImageInterpolation, QuarterTurn, RasterImageContent};
#[path = "convert_views_panel.rs"]
mod panel_convert;
use crate::state::{AxisProjection, AxisProjections, ProjectionSource};
use crate::state::{
    GroupMember, LayoutGroup, Panel, PanelId, PanelLabelMode, PanelLabelSpec, PanelLayout,
};
use panel_convert::*;

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
                    .iter()
                    .find(|dataset| dataset.resource_id() == d)
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
    datasets: &[Dataset],
) -> AxisProjections {
    AxisProjections {
        top: axis_projection_from_dto(dto.top.as_ref(), recipe_to_dataset, datasets),
        left: axis_projection_from_dto(dto.left.as_ref(), recipe_to_dataset, datasets),
    }
}
fn axis_projection_from_dto(
    dto: Option<&AxisProjectionDto>,
    recipe_to_dataset: &HashMap<String, usize>,
    datasets: &[Dataset],
) -> AxisProjection {
    let Some(dto) = dto else {
        return AxisProjection::default();
    };
    let source = match dto.source.as_str() {
        "attached" => dto
            .attached
            .as_ref()
            .and_then(|id| recipe_to_dataset.get(id).copied())
            .and_then(|index| datasets.get(index))
            .map(Dataset::resource_id)
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
                next_series_id: 0,
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
                parent_panel: canvas
                    .parent_panel(object.id)
                    .map(|id| ParentPanelDto::Panel { id: id.to_string() })
                    .unwrap_or(ParentPanelDto::Loose),
                viewport: None,
                axis_overrides: None,
                text: None,
                shape: None,
                image: None,
                locked: object.locked,
                visible: object.visible,
                snapshot: None,
            };
            match &object.kind {
                CanvasObjectKind::Plot(plot) => {
                    let primary = plot.primary_dataset().ok_or_else(|| {
                        ProjectError::Invalid(format!(
                            "view {view_id} plot {} has no primary dataset",
                            object.id
                        ))
                    })?;
                    let primary_dataset = datasets
                        .iter()
                        .find(|dataset| dataset.resource_id() == primary)
                        .ok_or_else(|| {
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
                        Dataset::Afm(_) => "heatmap",
                        Dataset::MassSpec(_) => "line_plot",
                        Dataset::Xrd(_) => "line_plot",
                        Dataset::Xps(_) => "line_plot",
                    };
                    let mut ids = std::collections::BTreeSet::new();
                    let series = plot
                        .binding
                        .series
                        .iter()
                        .map(|sb| {
                            if !ids.insert(sb.id) {
                                return Err(ProjectError::Invalid(format!(
                                    "view {view_id} plot {} has duplicate series id {}",
                                    object.id, sb.id
                                )));
                            }
                            let dataset = datasets
                                .iter()
                                .find(|dataset| dataset.resource_id() == sb.source.resource)
                                .ok_or_else(|| {
                                ProjectError::Invalid(format!(
                                    "view {view_id} plot {} references missing series dataset {}",
                                    object.id, sb.source.resource
                                ))
                            })?;
                            validate_series_source(
                                dataset,
                                sb.source.field,
                                sb.source.item,
                                &sb.encoding,
                                &format!("view {view_id} plot {} series {}", object.id, sb.id),
                            )?;
                            Ok(SeriesBindingDto {
                                id: sb.id.get(),
                                source: match sb.source.item {
                                    Some(item) => SeriesSourceDto::TraceItem {
                                        input: format!("recipe_{}", dataset.resource_id()),
                                        field: sb.source.field.get(),
                                        item,
                                    },
                                    None => SeriesSourceDto::Field {
                                        input: format!("recipe_{}", dataset.resource_id()),
                                        field: sb.source.field.get(),
                                    },
                                },
                                label: sb.label.clone(),
                                encoding: sb.encoding.clone(),
                                visible: sb.visible,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let stack = (plot.stack != StackSpec::default())
                        .then(|| StackDto::from_spec(&plot.stack));
                    Ok(ViewCanvasObject {
                        input: plot
                            .display_owner
                            .map(|owner| format!("recipe_{owner}"))
                            .unwrap_or_default(),
                        next_series_id: plot.next_series_id.get(),
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
                        axis_overrides: AxisOverridesDto::from_overrides(&plot.axis_overrides),
                        ..base(kind)
                    })
                }
                CanvasObjectKind::Text(t) => Ok(ViewCanvasObject {
                    text: Some(TextBoxDto::from_text_box(t)),
                    ..base("text")
                }),
                CanvasObjectKind::Shape(s) => Ok(ViewCanvasObject {
                    shape: Some(ShapeDto::from_shape(s)),
                    ..base("shape")
                }),
                CanvasObjectKind::RasterImage(image) => Ok(ViewCanvasObject {
                    image: Some(RasterImageDto {
                        asset: image.asset.to_string(),
                        page_index: image.page_index,
                        crop: image.crop,
                        fit: match image.fit {
                            ImageFit::Contain => "contain",
                            ImageFit::Cover => "cover",
                            ImageFit::Stretch => "stretch",
                        }
                        .to_owned(),
                        rotation: match image.rotation {
                            QuarterTurn::Zero => 0,
                            QuarterTurn::Clockwise90 => 90,
                            QuarterTurn::Clockwise180 => 180,
                            QuarterTurn::Clockwise270 => 270,
                        },
                        opacity: image.opacity,
                        interpolation: match image.interpolation {
                            ImageInterpolation::Auto => "auto",
                            ImageInterpolation::Nearest => "nearest",
                            ImageInterpolation::Linear => "linear",
                        }
                        .to_owned(),
                        preserve_aspect: image.preserve_aspect,
                    }),
                    ..base("raster_image")
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
        next_object_id: canvas.next_object_id.get(),
        next_panel_label_slot: canvas.next_panel_label_slot,
        caption: canvas.caption.clone(),
        caption_visible: canvas.caption_visible,
        panel_label_style: canvas.panel_label_style.as_key().to_owned(),
        panels: canvas.panels.iter().map(panel_to_dto).collect(),
        loose_item_order: canvas
            .objects
            .iter()
            .filter(|item| canvas.parent_panel(item.id).is_none())
            .map(|item| item.id.to_string())
            .collect(),
        groups: canvas.groups.iter().map(group_to_dto).collect(),
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
    canvas.panel_label_style = crate::state::PanelLabelStyle::try_from_key(&view.panel_label_style)
        .ok_or_else(|| {
            ProjectError::Invalid(format!(
                "unknown panel label style {}",
                view.panel_label_style
            ))
        })?;
    canvas.layout = view
        .layout
        .grid
        .map(PageLayoutDto::into_layout)
        .unwrap_or_default();
    if let Some([r, g, b]) = view.layout.background {
        canvas.background = plotx_figure::Color::rgb(r, g, b);
    }
    let mut max_id = 0;
    let mut declared_parents = std::collections::BTreeMap::new();
    for view_object in &view.objects {
        let object_id = view_object
            .id
            .parse::<ObjectId>()
            .map_err(|_| ProjectError::Invalid(format!("invalid object id {}", view_object.id)))?;
        let frame = view_object.frame.into_frame();
        let mut kind = match view_object.kind.as_str() {
            "text" => CanvasObjectKind::Text(text_box_from(view_object, false)),
            "shape" => CanvasObjectKind::Shape(
                view_object
                    .shape
                    .clone()
                    .map(ShapeDto::into_shape)
                    .unwrap_or_else(|| ShapeObject::new(ShapeKind::Rect)),
            ),
            "raster_image" => {
                let image = view_object.image.as_ref().ok_or_else(|| {
                    ProjectError::Invalid(format!(
                        "view {view_id} raster content {} has no image parameters",
                        view_object.id
                    ))
                })?;
                CanvasObjectKind::RasterImage(RasterImageContent {
                    asset: image.asset.parse::<AssetId>().map_err(|_| {
                        ProjectError::Invalid(format!("invalid asset id {}", image.asset))
                    })?,
                    page_index: image.page_index,
                    crop: image.crop,
                    fit: match image.fit.as_str() {
                        "contain" => ImageFit::Contain,
                        "cover" => ImageFit::Cover,
                        "stretch" => ImageFit::Stretch,
                        other => {
                            return Err(ProjectError::Invalid(format!(
                                "unknown image fit {other}"
                            )));
                        }
                    },
                    rotation: match image.rotation {
                        0 => QuarterTurn::Zero,
                        90 => QuarterTurn::Clockwise90,
                        180 => QuarterTurn::Clockwise180,
                        270 => QuarterTurn::Clockwise270,
                        other => {
                            return Err(ProjectError::Invalid(format!(
                                "invalid image rotation {other}"
                            )));
                        }
                    },
                    opacity: image.opacity,
                    interpolation: match image.interpolation.as_str() {
                        "auto" => ImageInterpolation::Auto,
                        "nearest" => ImageInterpolation::Nearest,
                        "linear" => ImageInterpolation::Linear,
                        other => {
                            return Err(ProjectError::Invalid(format!(
                                "unknown image interpolation {other}"
                            )));
                        }
                    },
                    preserve_aspect: image.preserve_aspect,
                })
            }
            "line_plot" | "contour_plot" | "stack_plot" | "plot" => {
                let resolve = |input: &str| {
                    recipe_to_dataset.get(input).copied().ok_or_else(|| {
                        ProjectError::Invalid(format!(
                            "view {view_id} references unknown recipe {input}"
                        ))
                    })
                };
                if view_object.series.is_empty() {
                    return Err(ProjectError::Invalid(format!(
                        "view {view_id} plot {} has no series bindings",
                        view_object.id
                    )));
                }
                let mut series = Vec::with_capacity(view_object.series.len());
                let mut ids = std::collections::BTreeSet::new();
                for sb in &view_object.series {
                    if !ids.insert(sb.id) {
                        return Err(ProjectError::Invalid(format!(
                            "view {view_id} plot {} has duplicate series id {}",
                            view_object.id, sb.id
                        )));
                    }
                    let (input, stored_field, item) = sb.source.parts();
                    let index = resolve(input)?;
                    let dataset = app.doc.datasets.get(index).ok_or_else(|| {
                        ProjectError::Invalid(format!(
                            "view {view_id} references unavailable dataset index {index}"
                        ))
                    })?;
                    let field = crate::state::FieldId::new(stored_field);
                    validate_series_source(
                        dataset,
                        field,
                        item,
                        &sb.encoding,
                        &format!("view {view_id} series {}", sb.id),
                    )?;
                    series.push(SeriesBinding {
                        id: SeriesId::new(sb.id),
                        source: crate::state::SeriesSource {
                            resource: dataset.resource_id(),
                            field,
                            item,
                        },
                        label: sb.label.clone(),
                        encoding: sb.encoding.clone(),
                        visible: sb.visible,
                    });
                }
                let binding = DataBinding { series };
                let display_owner = if view_object.input.is_empty() {
                    None
                } else {
                    Some(app.doc.datasets[resolve(&view_object.input)?].resource_id())
                };
                let stack = view_object
                    .stack
                    .clone()
                    .map(StackDto::into_spec)
                    .unwrap_or_default();
                let dataset_id = binding.primary_dataset().ok_or_else(|| {
                    ProjectError::Invalid(format!("view {} has an empty data binding", view.id))
                })?;
                let di = app.doc.dataset_index(dataset_id).ok_or_else(|| {
                    ProjectError::Invalid(format!(
                        "view {} references missing dataset {dataset_id}",
                        view.id
                    ))
                })?;
                // `dataset_index` above searched this same immutable vector.
                let domain = app.doc.datasets[di].domain();
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
                    .map(|dto| projections_from_dto(dto, recipe_to_dataset, &app.doc.datasets))
                    .unwrap_or_default();
                // A snapshot is a picture of a figure the document could still
                // produce. When the dataset's selected DOSY map did not survive
                // the load, it no longer can: replaying the snapshot would leave
                // the saved contours on the canvas while the load report says the
                // stack is being shown. Rebuild from the restored document
                // instead, so what is drawn and what is reported agree.
                let map_unavailable = app.doc.datasets[di]
                    .as_nmr2d()
                    .is_some_and(|dataset| dataset.missing_selected_map_note().is_some());
                let mut figure = match &view_object.snapshot {
                    Some(snapshot) if !map_unavailable => read_json(zip, &snapshot.figure)
                        .unwrap_or_else(|_| {
                            app.build_object_figure(
                                display_owner,
                                &binding,
                                &chart,
                                &stack,
                                &projections,
                                size_mm,
                            )
                        }),
                    _ => app.build_object_figure(
                        display_owner,
                        &binding,
                        &chart,
                        &stack,
                        &projections,
                        size_mm,
                    ),
                };
                let axis_overrides = view_object
                    .axis_overrides
                    .as_ref()
                    .map(AxisOverridesDto::to_overrides)
                    .unwrap_or_default();
                // Must track the branch actually taken above: a snapshot has axis
                // overrides and the viewport already baked in, a rebuilt figure
                // does not, so a bypassed snapshot has to be treated as absent
                // here or the rebuilt figure would lose both.
                let snapshot_backed = view_object.snapshot.is_some() && !map_unavailable;
                let derived_axes = if snapshot_backed {
                    let derived = app.build_object_figure(
                        display_owner,
                        &binding,
                        &chart,
                        &stack,
                        &projections,
                        size_mm,
                    );
                    crate::state::DerivedAxes::from_figure(&derived)
                } else {
                    crate::state::DerivedAxes::from_figure(&figure)
                };
                if !snapshot_backed {
                    axis_overrides.apply_to(&mut figure);
                }
                let mut viewport = view_object
                    .viewport
                    .as_ref()
                    .map(ViewportDto::to_viewport)
                    .unwrap_or_else(|| CanvasViewport::from_figure(&figure));
                if !snapshot_backed {
                    if axis_overrides.y_range.is_some() && figure.y.categories.is_none() {
                        viewport.auto_y = false;
                    }
                    viewport.sync_full_from(&figure);
                    viewport.apply_to(&mut figure);
                }
                figure.title.clear();
                CanvasObjectKind::Plot(Box::new(PlotObject::from_materialized_figure(
                    display_owner,
                    SeriesId::new(view_object.next_series_id),
                    binding,
                    chart,
                    stack,
                    projections,
                    axis_overrides,
                    derived_axes,
                    figure,
                    viewport,
                    PanelMeta::new(app.default_plot_title(di), frame.width),
                )))
            }
            other => {
                return Err(ProjectError::Invalid(format!(
                    "view {view_id} has unknown content kind {other}"
                )));
            }
        };
        if let CanvasObjectKind::Plot(plot) = &mut kind {
            plot.repair_series_allocator().ok_or_else(|| {
                ProjectError::Invalid(format!(
                    "view {view_id} object {} exhausts the series id space",
                    view_object.id
                ))
            })?;
        }
        canvas.objects.push(CanvasObject {
            id: object_id,
            name: view_object.name.clone(),
            frame,
            locked: view_object.locked,
            visible: view_object.visible,
            kind,
        });
        let parent = match &view_object.parent_panel {
            ParentPanelDto::Loose => None,
            ParentPanelDto::Panel { id } => Some(id.parse::<PanelId>().map_err(|_| {
                ProjectError::Invalid(format!(
                    "invalid parent panel id for content {}",
                    view_object.id
                ))
            })?),
        };
        if declared_parents.insert(object_id, parent).is_some() {
            return Err(ProjectError::Invalid(format!(
                "duplicate content id {}",
                view_object.id
            )));
        }
        max_id = max_id.max(object_id.get());
    }
    let repaired_next = ObjectId::new(max_id)
        .try_advance(1)
        .ok_or_else(|| ProjectError::Invalid("object id space exhausted".to_owned()))?;
    canvas.next_object_id = ObjectId::new(view.next_object_id).max(repaired_next);
    canvas.panels = view
        .panels
        .iter()
        .map(panel_from_dto)
        .collect::<Result<Vec<_>>>()?;
    canvas.groups = view
        .groups
        .iter()
        .map(group_from_dto)
        .collect::<Result<Vec<_>>>()?;
    canvas.next_group_id = canvas
        .groups
        .iter()
        .map(|group| group.id)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let loose: Vec<ObjectId> = view
        .loose_item_order
        .iter()
        .map(|id| {
            id.parse::<ObjectId>()
                .map_err(|_| ProjectError::Invalid(format!("invalid loose content id {id}")))
        })
        .collect::<Result<_>>()?;
    let mut seen_loose = std::collections::BTreeSet::new();
    for id in &loose {
        if !seen_loose.insert(*id) || canvas.object(*id).is_none() {
            return Err(ProjectError::Invalid(format!(
                "invalid or duplicate loose content id {id}"
            )));
        }
    }
    for item in &canvas.objects {
        let actual = canvas.parent_panel(item.id);
        let declared = declared_parents.get(&item.id).copied().flatten();
        if actual != declared {
            return Err(ProjectError::Invalid(format!(
                "content {} parent does not match panel order",
                item.id
            )));
        }
        if actual.is_none() != seen_loose.contains(&item.id) {
            return Err(ProjectError::Invalid(format!(
                "content {} loose order is inconsistent",
                item.id
            )));
        }
    }
    let minimum_label_slot = canvas
        .panels
        .iter()
        .filter_map(|panel| match panel.label.mode {
            PanelLabelMode::Auto { slot } => slot.checked_add(1),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    if view.next_panel_label_slot < minimum_label_slot {
        return Err(ProjectError::Invalid(format!(
            "next panel label slot {} does not exceed allocated auto slots",
            view.next_panel_label_slot
        )));
    }
    canvas.next_panel_label_slot = view.next_panel_label_slot;
    canvas.validate_structure().map_err(ProjectError::Invalid)?;
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
