use super::{Document, PlotxApp};

/// A document-level operation that replaces or removes the current project.
/// File imports are deliberately absent: they add to the current project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectTransition {
    New,
    Close,
    Open(std::path::PathBuf),
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectTransitionPhase {
    NeedsConfirmation,
    Saving,
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingProjectTransition {
    pub target: ProjectTransition,
    pub phase: ProjectTransitionPhase,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingProjectSave {
    pub path: std::path::PathBuf,
    pub include_view_snapshots: bool,
    /// Continue the pending project transition only after this save succeeds.
    pub continue_transition: bool,
}

impl PlotxApp {
    /// Record one persisted document mutation. The monotonically increasing
    /// generation lets background saves distinguish their captured revision
    /// from edits that happen while they are running.
    pub fn mark_document_dirty(&mut self) {
        self.doc.mark_dirty();
        self.session.project_present = true;
    }

    pub fn request_project_transition(&mut self, target: ProjectTransition) {
        let phase = if self.doc.dirty {
            ProjectTransitionPhase::NeedsConfirmation
        } else {
            ProjectTransitionPhase::Ready
        };
        self.session.ui.project_transition = Some(PendingProjectTransition { target, phase });
    }

    pub fn queue_project_save(
        &mut self,
        path: std::path::PathBuf,
        include_view_snapshots: bool,
        continue_transition: bool,
    ) {
        self.session.ui.pending_project_save = Some(PendingProjectSave {
            path,
            include_view_snapshots,
            continue_transition,
        });
        self.session.ui.project_save_in_progress = true;
        if continue_transition && let Some(transition) = self.session.ui.project_transition.as_mut()
        {
            transition.phase = ProjectTransitionPhase::Saving;
        }
    }

    pub fn start_new_project(&mut self) {
        let fresh = Self::new_with_settings(self.settings.clone());
        self.install_loaded_project(fresh);
        self.session.project_present = true;
        self.session.status = "New project ready.".to_owned();
    }

    pub fn close_project(&mut self) {
        let fresh = Self::new_with_settings(self.settings.clone());
        self.install_loaded_project(fresh);
        self.session.project_present = false;
        self.session.status = "Project closed.".to_owned();
    }
}

impl Document {
    pub(crate) fn mark_dirty(&mut self) {
        self.edit_generation = self
            .edit_generation
            .checked_add(1)
            .expect("document edit generation exhausted");
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_transition_waits_for_confirmation() {
        let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
        app.mark_document_dirty();
        app.request_project_transition(ProjectTransition::Close);

        let pending = app.session.ui.project_transition.as_ref().unwrap();
        assert_eq!(pending.target, ProjectTransition::Close);
        assert_eq!(pending.phase, ProjectTransitionPhase::NeedsConfirmation);
    }

    #[test]
    fn clean_transition_is_ready_immediately() {
        let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
        app.request_project_transition(ProjectTransition::New);

        assert_eq!(
            app.session.ui.project_transition.unwrap().phase,
            ProjectTransitionPhase::Ready
        );
    }

    #[test]
    fn new_and_close_have_distinct_welcome_state() {
        let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
        app.start_new_project();
        assert!(app.session.project_present);

        app.close_project();
        assert!(!app.session.project_present);
    }
}
