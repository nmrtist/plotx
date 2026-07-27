use super::*;
use crate::automation::{ResourceRef, TargetRef};
use crate::properties::ilt;
use crate::settings::{MAX_ILT_LAMBDA, MIN_ILT_LAMBDA, Settings};
use crate::state::{Dataset, Nmr2DDataset, PlotxApp};
use crate::{DosyInvocation, DosyResultProvenance, IltParams};
use num_complex::Complex64;
use plotx_io::{
    AxisSource, DiffusionMeta, Dim, Domain, NmrData2D, PseudoAxis, PseudoKind, QuadMode,
};
use std::path::PathBuf;

fn data() -> NmrData2D {
    let dim = Dim {
        spectral_width_hz: 4_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: "1H".to_owned(),
        group_delay: 0.0,
    };
    NmrData2D {
        data: vec![Complex64::new(1.0, 0.0); 16],
        rows: 4,
        cols: 4,
        domain: Domain::Frequency,
        direct: dim.clone(),
        indirect: dim,
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: Some("diffusion".to_owned()),
        pseudo_axis: Some(PseudoAxis {
            name: "g".to_owned(),
            kind: PseudoKind::Gradient,
            values: vec![0.1, 0.2, 0.3, 0.4],
            unit: "T/m".to_owned(),
            source: AxisSource::EmbeddedList,
        }),
        diffusion: Some(DiffusionMeta {
            gamma: 2.675_222e8,
            delta: 2e-3,
            big_delta: 0.1,
            tau: 0.0,
            shape_factor: 1.0 / 3.0,
        }),
        nus: None,
        source: "ILT property".to_owned(),
    }
}

pub(crate) fn ilt_app(lambda: f64) -> (PlotxApp, TargetRef) {
    let mut app = PlotxApp::new_with_settings(Settings::default());
    let mut dataset = Nmr2DDataset::load(data());
    dataset.ilt_provenance = Some(DosyResultProvenance {
        algorithm: "ilt_map".to_owned(),
        version: 1,
        input: DosyInvocation::Ilt {
            params: IltParams {
                lambda,
                ..IltParams::default()
            },
        },
        data_fingerprint: "fixture".to_owned(),
    });
    let id = dataset.resource_id;
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(dataset)));
    (app, TargetRef::resource(ResourceRef::from(id)))
}

fn temp_settings(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(format!("plotx-ilt-property-{name}-{}", std::process::id()))
}

#[test]
fn ilt_default_catalog_edit_uses_shared_bounds_and_persists() {
    let path = temp_settings("roundtrip").with_extension("json");
    let _ = std::fs::remove_file(&path);
    let mut app = PlotxApp::new_with_settings(Settings::default());
    let address = PropertyAddress::new(app.app_target(), ilt::DEFAULT_LAMBDA);
    let resolved = app.resolve_property(&address).expect("default reads");
    assert_eq!(
        resolved.schema,
        ResolvedSchema::Float {
            bounds: FloatBounds::inclusive(MIN_ILT_LAMBDA, MAX_ILT_LAMBDA),
            display: FloatDisplay::Log10("λ"),
        }
    );

    let commit = app
        .plan_property_write(
            ilt::DEFAULT_LAMBDA,
            std::slice::from_ref(&app.app_target()),
            &PropertyValue::Float(0.4),
        )
        .expect("legal lambda plans");
    app.commit_property_with_settings_writer(commit, |settings| {
        crate::settings::save_to_path(&path, settings)
    });
    let loaded = crate::settings::load_from_paths(&path, None);
    let _ = std::fs::remove_file(&path);
    assert_eq!(loaded.processing.ilt_lambda, 0.4);
}

#[test]
fn stored_ilt_lambda_reads_without_a_transaction_and_all_edits_stop_at_read_only_gate() {
    let (app, target) = ilt_app(0.07);
    let address = PropertyAddress::new(target.clone(), ilt::RESULT_LAMBDA);
    let resolved = app
        .resolve_property(&address)
        .expect("reading uses the provider directly, without a transaction");
    assert_eq!(
        resolved.value,
        AggregateValue::Uniform(PropertyValue::Float(0.07))
    );
    assert_eq!(resolved.availability, Availability::ReadOnly);
    assert!(resolved.default_value.is_none());
    // Assert on the address the provider echoed back, not on the `TargetRef` this
    // fixture built: the latter cannot fail for any change to the provider.
    assert!(resolved.address.target.component.is_none());
    assert_eq!(
        resolved.address.target.resource.kind.0,
        crate::automation::KIND_DATASET
    );
    assert_eq!(resolved.address.definition, ilt::RESULT_LAMBDA);

    let errors = [
        app.plan_property_write(
            ilt::RESULT_LAMBDA,
            std::slice::from_ref(&target),
            &PropertyValue::Float(0.2),
        )
        .expect_err("Set must be rejected"),
        app.plan_property_reset(ilt::RESULT_LAMBDA, std::slice::from_ref(&target))
            .expect_err("Reset must be rejected"),
        app.plan_property_step(
            ilt::RESULT_LAMBDA,
            std::slice::from_ref(&target),
            PropertyStep::Raise,
        )
        .expect_err("Step must be rejected"),
    ];
    for error in errors {
        assert_eq!(error, PropertyError::ReadOnly(ilt::RESULT_LAMBDA));
        assert!(error.to_string().contains("read-only"));
    }
}
