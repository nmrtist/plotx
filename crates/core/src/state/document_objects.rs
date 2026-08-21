use super::{CanvasDocument, CanvasObject, CraftRunId, DatasetId, ObjectId, XViewportLinkId};

impl CanvasDocument {
    pub fn object(&self, id: ObjectId) -> Option<&CanvasObject> {
        self.objects.iter().find(|object| object.id == id)
    }

    pub fn object_mut(&mut self, id: ObjectId) -> Option<&mut CanvasObject> {
        self.objects.iter_mut().find(|object| object.id == id)
    }

    pub fn first_plot_object_id(&self) -> Option<ObjectId> {
        self.objects
            .iter()
            .find(|object| object.plot().is_some())
            .map(|object| object.id)
    }

    pub fn selected_plot_object_id(&self) -> Option<ObjectId> {
        self.selected_object
            .and_then(|id| self.object(id).filter(|o| o.plot().is_some()).map(|_| id))
    }

    pub fn active_plot_object_id(&self) -> Option<ObjectId> {
        self.selected_object
            .and_then(|id| {
                self.object(id)
                    .filter(|object| object.plot().is_some())
                    .map(|_| id)
            })
            .or_else(|| self.first_plot_object_id())
    }

    pub fn active_dataset(&self) -> Option<DatasetId> {
        self.active_plot_object_id()
            .and_then(|id| self.object(id))
            .and_then(CanvasObject::dataset)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasAnalysisBinding {
    Craft { dataset: DatasetId, run: CraftRunId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XViewportLinkGroup {
    pub id: XViewportLinkId,
    pub members: Vec<ObjectId>,
}

impl CanvasDocument {
    pub fn linked_x_members(&self, object: ObjectId) -> &[ObjectId] {
        self.x_viewport_links
            .iter()
            .find(|group| group.members.contains(&object))
            .map(|group| group.members.as_slice())
            .unwrap_or(&[])
    }

    pub fn validate_x_viewport_links(&self) -> Result<(), String> {
        let plots = self
            .objects
            .iter()
            .filter(|object| object.plot().is_some())
            .map(|object| object.id)
            .collect::<std::collections::BTreeSet<_>>();
        let mut ids = std::collections::BTreeSet::new();
        let mut members = std::collections::BTreeSet::new();
        for group in &self.x_viewport_links {
            if !ids.insert(group.id) {
                return Err(format!("duplicate x viewport link id {}", group.id));
            }
            if group.members.len() < 2 {
                return Err("x viewport link groups require at least two plots".to_owned());
            }
            for member in &group.members {
                if !plots.contains(member) {
                    return Err(format!("x viewport link references missing plot {member}"));
                }
                if !members.insert(*member) {
                    return Err(format!(
                        "plot {member} belongs to multiple x viewport links"
                    ));
                }
            }
        }
        Ok(())
    }
}
