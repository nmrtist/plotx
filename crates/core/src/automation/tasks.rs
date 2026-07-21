use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(pub String);

pub enum TaskStart<T> {
    Completed(T),
    Scheduled(TaskId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskEvent {
    Started {
        id: TaskId,
        total: usize,
    },
    Progress {
        id: TaskId,
        completed: usize,
        total: usize,
        message: String,
    },
    Failed {
        id: TaskId,
        message: String,
    },
    Finished {
        id: TaskId,
        cancelled: bool,
    },
}

#[derive(Clone, Default)]
pub struct TaskCancellation(Arc<AtomicBool>);

impl TaskCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

struct ManagedTask {
    cancellation: TaskCancellation,
    events: mpsc::Receiver<TaskEvent>,
}

/// Shared task lifecycle for immutable worker snapshots. Document commits stay
/// in the caller after it receives the computed result and rechecks revision.
#[derive(Default)]
pub struct TaskManager {
    tasks: BTreeMap<TaskId, ManagedTask>,
}

impl TaskManager {
    pub fn spawn(
        &mut self,
        work: impl FnOnce(TaskId, TaskCancellation, mpsc::Sender<TaskEvent>) + Send + 'static,
    ) -> TaskId {
        let id = TaskId(uuid::Uuid::new_v4().to_string());
        let cancellation = TaskCancellation::default();
        let (sender, events) = mpsc::channel();
        let worker_id = id.clone();
        let worker_cancellation = cancellation.clone();
        std::thread::spawn(move || work(worker_id, worker_cancellation, sender));
        self.tasks.insert(
            id.clone(),
            ManagedTask {
                cancellation,
                events,
            },
        );
        id
    }

    pub fn cancel(&self, id: &TaskId) -> bool {
        self.tasks.get(id).is_some_and(|task| {
            task.cancellation.cancel();
            true
        })
    }

    pub fn poll(&mut self, id: &TaskId) -> Vec<TaskEvent> {
        let Some(task) = self.tasks.get(id) else {
            return Vec::new();
        };
        task.events.try_iter().collect()
    }

    pub fn forget(&mut self, id: &TaskId) {
        self.tasks.remove(id);
    }
}
