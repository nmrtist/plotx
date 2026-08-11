use super::*;

pub(crate) fn large_image_consent_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let awaiting = MANAGER.with(|manager| {
        manager
            .borrow()
            .jobs
            .iter()
            .position(|job| job.state == ImportImageState::AwaitingLargeImageConsent)
    });
    let Some(index) = awaiting else { return };
    let mut decision = None;
    egui::Window::new("Large image")
        .id(egui::Id::new("large-image-import-consent"))
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label("This image exceeds 100 MP or an estimated 512 MiB when decoded.");
            ui.label("PlotX will embed the original and use a bounded proxy for editing.");
            ui.horizontal(|ui| {
                if ui.button("Continue with proxy").clicked() {
                    decision = Some(true);
                }
                if ui.button("Cancel import").clicked() {
                    decision = Some(false);
                }
            });
        });
    if let Some(allow) = decision {
        MANAGER.with(|manager| {
            if let Some(job) = manager.borrow_mut().jobs.get_mut(index)
                && job.consent.send(allow).is_ok()
            {
                job.state = if allow {
                    ImportImageState::DecodingProxy
                } else {
                    job.cancelled.store(true, Ordering::Relaxed);
                    ImportImageState::Cancelled
                };
            }
        });
        app.session.status = if allow {
            "Generating a bounded image proxy…".to_owned()
        } else {
            "Image import cancelled.".to_owned()
        };
    }
}
