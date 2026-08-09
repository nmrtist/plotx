use super::{PropertyTransaction, TargetSnapshot};
use crate::actions::Action;
use crate::state::{ChartSpec, ObjectId, ObjectStyle, PanelMeta, PlotxApp, StackSpec};

type ObjectFlags = (bool, bool);
type ObjectFlagPlan = (usize, ObjectId, ObjectFlags, ObjectFlags);

#[derive(Default)]
pub(super) struct ObjectPlans {
    stacks: Vec<(usize, ObjectId, StackSpec, StackSpec)>,
    charts: Vec<(usize, ObjectId, ChartSpec, ChartSpec)>,
    panels: Vec<(usize, ObjectId, PanelMeta, PanelMeta)>,
    flags: Vec<ObjectFlagPlan>,
    styles: Vec<(usize, ObjectId, ObjectStyle, ObjectStyle)>,
}

pub(super) enum ObjectTargetSnapshot {
    Stack(usize, ObjectId, StackSpec),
    Chart(usize, ObjectId, ChartSpec),
    Panel(usize, ObjectId, PanelMeta),
    Flags(usize, ObjectId, ObjectFlags),
    Style(usize, ObjectId, ObjectStyle),
}

impl ObjectPlans {
    pub(super) fn is_empty(&self) -> bool {
        self.stacks.is_empty()
            && self.charts.is_empty()
            && self.panels.is_empty()
            && self.flags.is_empty()
            && self.styles.is_empty()
    }

    pub(super) fn target_changed(&self, snapshot: &ObjectTargetSnapshot) -> bool {
        match snapshot {
            ObjectTargetSnapshot::Stack(canvas, object, before) => self
                .stacks
                .iter()
                .find(|entry| entry.0 == *canvas && entry.1 == *object)
                .is_some_and(|entry| entry.3 != *before),
            ObjectTargetSnapshot::Chart(canvas, object, before) => self
                .charts
                .iter()
                .find(|entry| entry.0 == *canvas && entry.1 == *object)
                .is_some_and(|entry| entry.3 != *before),
            ObjectTargetSnapshot::Panel(canvas, object, before) => self
                .panels
                .iter()
                .find(|entry| entry.0 == *canvas && entry.1 == *object)
                .is_some_and(|entry| entry.3 != *before),
            ObjectTargetSnapshot::Flags(canvas, object, before) => self
                .flags
                .iter()
                .find(|entry| entry.0 == *canvas && entry.1 == *object)
                .is_some_and(|entry| entry.3 != *before),
            ObjectTargetSnapshot::Style(canvas, object, before) => self
                .styles
                .iter()
                .find(|entry| entry.0 == *canvas && entry.1 == *object)
                .is_some_and(|entry| entry.3 != *before),
        }
    }

    pub(super) fn rollback(&mut self, snapshot: &ObjectTargetSnapshot) {
        match snapshot {
            ObjectTargetSnapshot::Stack(canvas, object, before) => {
                if let Some(entry) = self
                    .stacks
                    .iter_mut()
                    .find(|entry| entry.0 == *canvas && entry.1 == *object)
                {
                    entry.3 = *before;
                }
            }
            ObjectTargetSnapshot::Chart(canvas, object, before) => {
                if let Some(entry) = self
                    .charts
                    .iter_mut()
                    .find(|entry| entry.0 == *canvas && entry.1 == *object)
                {
                    entry.3 = before.clone();
                }
            }
            ObjectTargetSnapshot::Panel(canvas, object, before) => {
                if let Some(entry) = self
                    .panels
                    .iter_mut()
                    .find(|entry| entry.0 == *canvas && entry.1 == *object)
                {
                    entry.3 = before.clone();
                }
            }
            ObjectTargetSnapshot::Flags(canvas, object, before) => {
                if let Some(entry) = self
                    .flags
                    .iter_mut()
                    .find(|entry| entry.0 == *canvas && entry.1 == *object)
                {
                    entry.3 = *before;
                }
            }
            ObjectTargetSnapshot::Style(canvas, object, before) => {
                if let Some(entry) = self
                    .styles
                    .iter_mut()
                    .find(|entry| entry.0 == *canvas && entry.1 == *object)
                {
                    entry.3 = before.clone();
                }
            }
        }
    }

    pub(super) fn into_actions(self) -> Vec<Action> {
        let mut actions = Vec::new();
        actions.extend(
            self.stacks
                .into_iter()
                .filter(|(_, _, before, after)| before != after)
                .map(|(canvas, object, before, after)| {
                    Action::set_stack_spec(canvas, object, before, after)
                }),
        );
        actions.extend(
            self.charts
                .into_iter()
                .filter(|(_, _, before, after)| before != after)
                .map(|(canvas, object, before, after)| {
                    Action::set_chart_type(canvas, object, before, after)
                }),
        );
        actions.extend(
            self.panels
                .into_iter()
                .filter(|(_, _, before, after)| before != after)
                .map(|(canvas, object, before, after)| {
                    Action::set_panel_meta(canvas, object, before, after)
                }),
        );
        actions.extend(
            self.flags
                .into_iter()
                .filter(|(_, _, before, after)| before != after)
                .map(|(canvas, object, before, after)| {
                    Action::set_object_flags(canvas, object, before, after)
                }),
        );
        actions.extend(
            self.styles
                .into_iter()
                .filter(|(_, _, before, after)| before != after)
                .map(|(canvas, object, before, after)| {
                    Action::set_object_style(canvas, vec![(object, before)], vec![(object, after)])
                }),
        );
        actions
    }
}

impl PropertyTransaction {
    pub(crate) fn stack_spec(
        &mut self,
        app: &PlotxApp,
        canvas: usize,
        object: ObjectId,
    ) -> Result<&mut StackSpec, crate::properties::PropertyError> {
        let index = if let Some(index) = self
            .objects
            .stacks
            .iter()
            .position(|entry| entry.0 == canvas && entry.1 == object)
        {
            index
        } else {
            let current = plot(app, canvas, object)?.stack;
            self.objects.stacks.push((canvas, object, current, current));
            self.objects.stacks.len() - 1
        };
        if !has_object_snapshot(&self.target_before, canvas, object, "stack") {
            self.target_before
                .push(TargetSnapshot::Object(ObjectTargetSnapshot::Stack(
                    canvas,
                    object,
                    self.objects.stacks[index].3,
                )));
        }
        Ok(&mut self.objects.stacks[index].3)
    }

    pub(crate) fn chart_spec(
        &mut self,
        app: &PlotxApp,
        canvas: usize,
        object: ObjectId,
    ) -> Result<&mut ChartSpec, crate::properties::PropertyError> {
        let index = if let Some(index) = self
            .objects
            .charts
            .iter()
            .position(|entry| entry.0 == canvas && entry.1 == object)
        {
            index
        } else {
            let current = plot(app, canvas, object)?.chart.clone();
            self.objects
                .charts
                .push((canvas, object, current.clone(), current));
            self.objects.charts.len() - 1
        };
        if !has_object_snapshot(&self.target_before, canvas, object, "chart") {
            self.target_before
                .push(TargetSnapshot::Object(ObjectTargetSnapshot::Chart(
                    canvas,
                    object,
                    self.objects.charts[index].3.clone(),
                )));
        }
        Ok(&mut self.objects.charts[index].3)
    }

    pub(crate) fn panel_meta(
        &mut self,
        app: &PlotxApp,
        canvas: usize,
        object: ObjectId,
    ) -> Result<&mut PanelMeta, crate::properties::PropertyError> {
        let index = if let Some(index) = self
            .objects
            .panels
            .iter()
            .position(|entry| entry.0 == canvas && entry.1 == object)
        {
            index
        } else {
            let page = app.doc.canvases.get(canvas).ok_or_else(|| {
                crate::properties::PropertyError::UnknownTarget(format!("canvas {canvas}"))
            })?;
            let panel = page
                .parent_panel(object)
                .and_then(|id| page.panel(id))
                .ok_or_else(|| {
                    crate::properties::PropertyError::NotApplicable(
                        "The plot is not inside a panel.".to_owned(),
                    )
                })?;
            let current = PanelMeta::from_panel(panel);
            self.objects
                .panels
                .push((canvas, object, current.clone(), current));
            self.objects.panels.len() - 1
        };
        if !has_object_snapshot(&self.target_before, canvas, object, "panel") {
            self.target_before
                .push(TargetSnapshot::Object(ObjectTargetSnapshot::Panel(
                    canvas,
                    object,
                    self.objects.panels[index].3.clone(),
                )));
        }
        Ok(&mut self.objects.panels[index].3)
    }

    pub(crate) fn object_flags(
        &mut self,
        app: &PlotxApp,
        canvas: usize,
        object: ObjectId,
    ) -> Result<&mut (bool, bool), crate::properties::PropertyError> {
        let index = if let Some(index) = self
            .objects
            .flags
            .iter()
            .position(|entry| entry.0 == canvas && entry.1 == object)
        {
            index
        } else {
            let current = canvas_object(app, canvas, object)?;
            let flags = (current.visible, current.locked);
            self.objects.flags.push((canvas, object, flags, flags));
            self.objects.flags.len() - 1
        };
        if !has_object_snapshot(&self.target_before, canvas, object, "flags") {
            self.target_before
                .push(TargetSnapshot::Object(ObjectTargetSnapshot::Flags(
                    canvas,
                    object,
                    self.objects.flags[index].3,
                )));
        }
        Ok(&mut self.objects.flags[index].3)
    }

    pub(crate) fn object_style(
        &mut self,
        app: &PlotxApp,
        canvas: usize,
        object: ObjectId,
    ) -> Result<&mut ObjectStyle, crate::properties::PropertyError> {
        let index = if let Some(index) = self
            .objects
            .styles
            .iter()
            .position(|entry| entry.0 == canvas && entry.1 == object)
        {
            index
        } else {
            let current = canvas_object(app, canvas, object)?.style().ok_or_else(|| {
                crate::properties::PropertyError::UnknownTarget(object.to_string())
            })?;
            self.objects
                .styles
                .push((canvas, object, current.clone(), current));
            self.objects.styles.len() - 1
        };
        if !has_object_snapshot(&self.target_before, canvas, object, "style") {
            self.target_before
                .push(TargetSnapshot::Object(ObjectTargetSnapshot::Style(
                    canvas,
                    object,
                    self.objects.styles[index].3.clone(),
                )));
        }
        Ok(&mut self.objects.styles[index].3)
    }
}

fn has_object_snapshot(
    snapshots: &[TargetSnapshot],
    canvas: usize,
    object: ObjectId,
    kind: &str,
) -> bool {
    snapshots.iter().any(|snapshot| {
        let TargetSnapshot::Object(snapshot) = snapshot else {
            return false;
        };
        match snapshot {
            ObjectTargetSnapshot::Stack(c, o, _) => kind == "stack" && *c == canvas && *o == object,
            ObjectTargetSnapshot::Chart(c, o, _) => kind == "chart" && *c == canvas && *o == object,
            ObjectTargetSnapshot::Panel(c, o, _) => kind == "panel" && *c == canvas && *o == object,
            ObjectTargetSnapshot::Flags(c, o, _) => kind == "flags" && *c == canvas && *o == object,
            ObjectTargetSnapshot::Style(c, o, _) => kind == "style" && *c == canvas && *o == object,
        }
    })
}

fn canvas_object(
    app: &PlotxApp,
    canvas: usize,
    object: ObjectId,
) -> Result<&crate::state::CanvasObject, crate::properties::PropertyError> {
    app.doc
        .canvases
        .get(canvas)
        .and_then(|canvas| canvas.object(object))
        .ok_or_else(|| crate::properties::PropertyError::UnknownTarget(object.to_string()))
}

fn plot(
    app: &PlotxApp,
    canvas: usize,
    object: ObjectId,
) -> Result<&crate::state::PlotObject, crate::properties::PropertyError> {
    canvas_object(app, canvas, object)?
        .plot()
        .ok_or_else(|| crate::properties::PropertyError::UnknownTarget(object.to_string()))
}
