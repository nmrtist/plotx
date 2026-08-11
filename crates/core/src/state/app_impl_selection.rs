use super::*;

impl PlotxApp {
    pub fn set_hierarchical_paths(
        &mut self,
        ci: usize,
        paths: &[SelectionPath],
        additive: bool,
    ) -> Result<(), &'static str> {
        let mut selection = if additive {
            self.session.ui.hierarchical_selection.clone()
        } else {
            HierarchicalSelection::default()
        };
        for path in paths {
            selection.extend_sibling(*path)?;
        }
        self.session.ui.hierarchical_selection = selection;
        let ids: Vec<_> = self
            .session
            .ui
            .hierarchical_selection
            .paths()
            .iter()
            .filter_map(|path| path.content)
            .collect();
        self.session.ui.selection = if ids.is_empty() {
            Selection::None
        } else {
            Selection::Objects(ids.clone())
        };
        if let Some(canvas) = self.doc.canvases.get_mut(ci) {
            canvas.selected_object = ids.first().copied();
        }
        Ok(())
    }

    pub fn select_panel(&mut self, ci: usize, panel: PanelId) {
        let Some(canvas) = self.doc.canvases.get(ci) else {
            return;
        };
        if canvas.panel(panel).is_none() {
            return;
        }
        self.session.ui.panel_label_selection = None;
        self.session.ui.selection = Selection::None;
        self.session
            .ui
            .hierarchical_selection
            .replace(SelectionPath::panel(canvas.resource_id, panel));
        if let Some(canvas) = self.doc.canvases.get_mut(ci) {
            canvas.selected_object = None;
        }
    }

    pub fn toggle_panel_sibling(&mut self, ci: usize, panel: PanelId) -> Result<(), &'static str> {
        let Some(canvas) = self.doc.canvases.get(ci) else {
            return Err("The target page no longer exists.");
        };
        if canvas.panel(panel).is_none() {
            return Err("The target panel no longer exists.");
        }
        self.session
            .ui
            .hierarchical_selection
            .toggle_sibling(SelectionPath::panel(canvas.resource_id, panel))?;
        self.session.ui.selection = Selection::None;
        if let Some(canvas) = self.doc.canvases.get_mut(ci) {
            canvas.selected_object = None;
        }
        Ok(())
    }

    pub fn select_content(&mut self, ci: usize, id: ContentId) {
        let Some(canvas) = self.doc.canvases.get(ci) else {
            return;
        };
        if canvas.object(id).is_none() {
            return;
        }
        let canvas_id = canvas.resource_id;
        let parent = canvas.parent_panel(id);
        self.session.ui.panel_label_selection = None;
        self.session.ui.selection = Selection::Objects(self.group_click_members(ci, id));
        self.session
            .ui
            .hierarchical_selection
            .replace(SelectionPath::content(canvas_id, parent, id));
        if let Some(canvas) = self.doc.canvases.get_mut(ci) {
            canvas.selected_object = Some(id);
        }
    }

    pub fn toggle_content_sibling(&mut self, ci: usize, id: ContentId) -> Result<(), &'static str> {
        let Some(canvas) = self.doc.canvases.get(ci) else {
            return Err("The target page no longer exists.");
        };
        let path = SelectionPath::content(canvas.resource_id, canvas.parent_panel(id), id);
        self.session
            .ui
            .hierarchical_selection
            .toggle_sibling(path)?;
        let ids: Vec<_> = self
            .session
            .ui
            .hierarchical_selection
            .paths()
            .iter()
            .filter_map(|path| path.content)
            .collect();
        self.session.ui.selection = if ids.is_empty() {
            Selection::None
        } else {
            Selection::Objects(ids.clone())
        };
        if let Some(canvas) = self.doc.canvases.get_mut(ci) {
            canvas.selected_object = ids.first().copied();
        }
        Ok(())
    }

    pub fn enter_panel(&mut self, ci: usize, panel: PanelId) {
        let Some(canvas) = self.doc.canvases.get(ci) else {
            return;
        };
        let Some(first) = canvas
            .panel(panel)
            .and_then(|panel| panel.item_order.first())
            .copied()
        else {
            self.select_panel(ci, panel);
            return;
        };
        self.session
            .ui
            .hierarchical_selection
            .replace(SelectionPath::content(
                canvas.resource_id,
                Some(panel),
                first,
            ));
        self.session.ui.selection = Selection::single(first);
        if let Some(canvas) = self.doc.canvases.get_mut(ci) {
            canvas.selected_object = Some(first);
        }
    }

    pub fn exit_panel_scope(&mut self) {
        self.session.ui.hierarchical_selection.exit_scope();
        self.session.ui.selection = Selection::None;
        if let Some(ci) = self.session.active_canvas
            && let Some(canvas) = self.doc.canvases.get_mut(ci)
        {
            canvas.selected_object = None;
        }
    }
}
