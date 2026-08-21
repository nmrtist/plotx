use plotx_core::state::CraftAnalysisIntent;
use plotx_core::state::PlotxApp;

use super::{CommandId, requires};

pub(super) fn gate(app: &PlotxApp, command: CommandId) -> Result<(), &'static str> {
    let target = app
        .session
        .ui
        .craft_task_dataset
        .and_then(|id| app.doc.dataset_index(id));
    match command {
        CommandId::Craft => requires(
            app.active_dataset().is_some_and(|index| {
                app.doc.datasets[index]
                    .as_nmr()
                    .is_some_and(|nmr| nmr.data.domain == plotx_io::Domain::Time)
            }),
            "Select a one-dimensional time-domain NMR FID before opening CRAFT.",
        ),
        CommandId::RunCraft => requires(
            target.is_some_and(|index| {
                app.doc.datasets[index]
                    .as_nmr()
                    .is_some_and(|nmr| nmr.data.domain == plotx_io::Domain::Time)
            }),
            "Open CRAFT for a one-dimensional time-domain NMR FID before running it.",
        )
        .and_then(|()| {
            let can_run = target.is_some_and(|index| {
                let nmr = app.doc.datasets[index].as_nmr().unwrap();
                if let Some(cache) = &app.session.ui.craft_resolution_cache
                    && cache.dataset == nmr.resource_id
                    && cache.dataset_epoch == app.session.dataset_epoch
                    && cache.reference == nmr.craft_reference()
                    && cache.overrides == app.session.ui.craft_overrides
                    && cache.parent_run == app.session.ui.craft_base_run
                {
                    return cache.invocation.assessment.can_run();
                }
                plotx_processing::craft::resolve_craft_invocation(
                    &nmr.data,
                    nmr.craft_reference(),
                    &app.session.ui.craft_overrides,
                    app.session
                        .ui
                        .craft_base_run
                        .and_then(|id| nmr.craft_run(id).map(|run| &run.provenance.invocation)),
                )
                .assessment
                .can_run()
            });
            requires(
                can_run,
                "Resolve the CRAFT input errors shown in Setup before running it.",
            )
        })
        .and_then(|()| {
            let selected_count = target.map_or(0, |index| {
                let nmr = app.doc.datasets[index].as_nmr().unwrap();
                let invocation = plotx_processing::craft::resolve_craft_invocation(
                    &nmr.data,
                    nmr.craft_reference(),
                    &app.session.ui.craft_overrides,
                    app.session
                        .ui
                        .craft_base_run
                        .and_then(|id| nmr.craft_run(id).map(|run| &run.provenance.invocation)),
                );
                if invocation.sources.regions
                    == plotx_processing::craft::CraftParamSource::InputDerived
                {
                    0
                } else {
                    invocation.params.regions.len()
                }
            });
            let ready = match app.session.ui.craft_analysis_intent {
                CraftAnalysisIntent::ExploreBandwidth => true,
                CraftAnalysisIntent::SelectedSignals => selected_count > 0,
                CraftAnalysisIntent::CompareTwoSignals => selected_count == 2,
            };
            requires(
                ready,
                "Select the signal groups required by the analysis goal.",
            )
        })
        .and_then(|()| {
            requires(
                target.is_some_and(|index| {
                    let id = app.doc.datasets[index].resource_id();
                    app.session.compute.blocking_work_for(id).is_none()
                }),
                "Wait for the running computation on this dataset to finish.",
            )
        }),
        CommandId::CraftComponentTable => requires(
            target.is_some_and(|index| {
                app.session.ui.craft_selected_run.is_some_and(|run| {
                    app.doc.datasets[index]
                        .as_nmr()
                        .and_then(|nmr| nmr.craft_run(run))
                        .is_some()
                })
            }),
            "Select a completed CRAFT run before creating its data table.",
        ),
        _ => unreachable!("CRAFT gate called for a different command"),
    }
}
