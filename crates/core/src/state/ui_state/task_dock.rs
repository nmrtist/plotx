use super::UiState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskDockTab {
    Processing,
    Regions,
    CurveFit,
    Statistics,
}

impl UiState {
    pub fn open_task_tab(&mut self, tab: TaskDockTab) {
        self.task_dock_active = Some(tab);
    }

    pub fn close_task_tab(&mut self, tab: TaskDockTab) {
        match tab {
            TaskDockTab::Processing => {
                self.processing_task_dataset = None;
                self.processing_task_collapsed = false;
                self.proc_expanded_step = None;
            }
            TaskDockTab::Regions => {
                self.region_task_dataset = None;
                self.region_task_collapsed = false;
            }
            TaskDockTab::CurveFit => {
                self.curve_fit_task_dataset = None;
                self.curve_fit_task_collapsed = false;
            }
            TaskDockTab::Statistics => {
                self.stat_task_dataset = None;
                self.stat_task_collapsed = false;
                self.stat_draft = None;
            }
        }
        if self.task_dock_active == Some(tab) {
            self.task_dock_active = [
                (
                    TaskDockTab::Processing,
                    self.processing_task_dataset.is_some(),
                ),
                (TaskDockTab::Regions, self.region_task_dataset.is_some()),
                (TaskDockTab::CurveFit, self.curve_fit_task_dataset.is_some()),
                (TaskDockTab::Statistics, self.stat_task_dataset.is_some()),
            ]
            .into_iter()
            .find_map(|(candidate, open)| open.then_some(candidate));
        }
    }

    /// Discard every transient page in the shared canvas task dock.
    pub fn close_task_cards(&mut self) {
        self.processing_task_dataset = None;
        self.processing_task_collapsed = false;
        self.region_task_dataset = None;
        self.region_task_collapsed = false;
        self.curve_fit_task_dataset = None;
        self.curve_fit_task_collapsed = false;
        self.stat_task_dataset = None;
        self.stat_task_collapsed = false;
        self.stat_draft = None;
        self.task_dock_active = None;
    }
}
