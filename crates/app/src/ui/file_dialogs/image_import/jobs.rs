use super::*;

pub(crate) fn poll(app: &mut PlotxApp, ctx: &egui::Context) {
    proxy::poll_rebuilds(app, ctx);
    MANAGER.with(|manager| {
        let mut manager = manager.borrow_mut();
        let mut completed = Vec::new();
        for (index, job) in manager.jobs.iter_mut().enumerate() {
            loop {
                let event = match job.receiver.try_recv() {
                    Ok(event) => event,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        job.state = ImportImageState::Failed;
                        record_failures(
                            app,
                            job.operation,
                            0,
                            vec![failure(
                                "<batch>",
                                "unknown",
                                "worker",
                                "the image import worker stopped before returning a result"
                                    .to_owned(),
                                "Retry the import; if it still fails, review the diagnostic history.",
                            )],
                        );
                        completed.push(index);
                        break;
                    }
                };
                match event {
                    WorkerEvent::State(state) => {
                        job.state = state;
                        app.session.status = format!("Image import: {state:?}.");
                        if state == ImportImageState::Cancelled {
                            completed.push(index);
                            break;
                        }
                    }
                    WorkerEvent::Finished(results) => {
                        job.state = ImportImageState::ReadyToCommit;
                        commit(app, job, results);
                        completed.push(index);
                        break;
                    }
                }
            }
        }
        completed.sort_unstable();
        completed.dedup();
        for index in completed.into_iter().rev() {
            manager.jobs.remove(index);
        }
        if !manager.jobs.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    });
}
