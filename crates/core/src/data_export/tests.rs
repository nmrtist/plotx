use super::*;
use crate::state::{
    FloatSeries, PeakOrigin, StoredCurveFitAnalysis, materialized_float_series_table,
};
use crate::{BaselineMode, IntegralMethod};

fn request(content: DataExportContent) -> DataExportRequest {
    DataExportRequest {
        content,
        channel: IntensityChannel::Real,
        shape: TableShape::Matrix,
    }
}

fn snapshot(data: SnapshotData, content: DataExportContent) -> DataExportSnapshot {
    DataExportSnapshot {
        dataset_name: "A / dataset".into(),
        request: request(content),
        data,
    }
}

#[test]
fn one_dimensional_channels_use_processed_complex_values() {
    let mut value = snapshot(
        SnapshotData::Nmr1D {
            ppm: vec![1.0],
            values: vec![Complex64::new(3.0, 4.0)],
        },
        DataExportContent::ProcessedData,
    );
    assert_eq!(
        value.to_text(Delimiter::Comma).unwrap(),
        "ppm,intensity\n1,3\n"
    );
    value.request.channel = IntensityChannel::Imaginary;
    assert_eq!(
        value.to_text(Delimiter::Comma).unwrap(),
        "ppm,intensity\n1,4\n"
    );
    value.request.channel = IntensityChannel::Magnitude;
    assert_eq!(
        value.to_text(Delimiter::Comma).unwrap(),
        "ppm,intensity\n1,5\n"
    );
}

#[test]
fn complete_table_interleaves_sigma_and_leaves_missing_values_empty() {
    let typed = materialized_float_series_table(
        ("Time".into(), "s".into(), vec![Some(1.0), None, None]),
        vec![
            FloatSeries {
                name: "signal, one".into(),
                unit: String::new(),
                values: vec![Some(2.0), Some(3.0), Some(4.0)],
                uncertainty: Some(vec![Some(0.1), None, None]),
                fit: None,
            },
            FloatSeries {
                name: "二".into(),
                unit: String::new(),
                values: vec![Some(f64::INFINITY), Some(6.0), None],
                uncertainty: None,
                fit: None,
            },
        ],
        "plotx.test.export-table.v1",
    )
    .unwrap()
    .typed_state;
    let text = snapshot(
        SnapshotData::Table(Box::new(typed)),
        DataExportContent::TypedTable,
    )
    .to_text(Delimiter::Comma)
    .unwrap();
    assert_eq!(
        text,
        "Time,\"signal, one\",\"signal, one uncertainty\",二\n1,2,0.1,+Inf\n,3,,6\n,4,,\n"
    );
}

#[test]
fn true_2d_matrix_and_long_keep_row_major_axis_order() {
    let spectrum = Arc::new(Spectrum2D {
        f2_ppm: vec![10.0, 20.0],
        f1_ppm: vec![1.0, 2.0],
        data: vec![
            Complex64::new(11.0, 0.0),
            Complex64::new(12.0, 0.0),
            Complex64::new(21.0, 0.0),
            Complex64::new(22.0, 0.0),
        ],
        f2_size: 2,
        f1_size: 2,
        direct: plotx_processing::AxisMeta {
            nucleus: "1H".into(),
            observe_freq_mhz: 400.0,
        },
        indirect: plotx_processing::AxisMeta {
            nucleus: "1H".into(),
            observe_freq_mhz: 400.0,
        },
        source: String::new(),
    });
    let mut value = snapshot(
        SnapshotData::True2D(spectrum),
        DataExportContent::ProcessedData,
    );
    assert_eq!(
        value.to_text(Delimiter::Comma).unwrap(),
        "F1/F2 (ppm),10,20\n1,11,12\n2,21,22\n"
    );
    value.request.shape = TableShape::Long;
    assert_eq!(
        value.to_text(Delimiter::Comma).unwrap(),
        "f1_ppm,f2_ppm,intensity\n1,10,11\n1,20,12\n2,10,21\n2,20,22\n"
    );
}

#[test]
fn fit_parameters_keep_long_header_and_escape_names() {
    use plotx_analysis::fit_model::{FitDataset, FitOptions};
    use std::collections::BTreeMap;
    let x: Vec<f64> = (0..5).map(|value| value as f64).collect();
    let y: Vec<f64> = x.iter().map(|value| 2.0 + 3.0 * value).collect();
    let model = plotx_analysis::models::builtin_model_by_name("Linear").unwrap();
    let result = plotx_analysis::fit_model::fit_model(
        model,
        vec![FitDataset {
            id: "column-0".into(),
            inputs: BTreeMap::from([("x".into(), x.clone())]),
            responses: BTreeMap::from([("y".into(), y.clone())]),
            sigmas: BTreeMap::new(),
            constants: BTreeMap::new(),
        }],
        &[],
        FitOptions::default(),
    )
    .unwrap();
    let analyses = vec![StoredCurveFitAnalysis {
        id: 0,
        name: "curve, A".into(),
        bindings: Vec::new(),
        result,
        selection: None,
        plot_samples: BTreeMap::new(),
    }];
    let text = snapshot(SnapshotData::Fits(analyses), DataExportContent::CurveFits)
        .to_text(Delimiter::Comma)
        .unwrap();
    assert!(text.starts_with("record_type,analysis,dataset,response,model,name,value,standard_error,row,row_id,observed,predicted,residual,related_name,details\nparameter,\"curve, A\","));
}

#[test]
fn default_name_is_stable_and_descriptive() {
    let value = snapshot(
        SnapshotData::Nmr1D {
            ppm: vec![1.0],
            values: vec![Complex64::new(2.0, 0.0)],
        },
        DataExportContent::ProcessedData,
    );
    assert_eq!(
        value.default_file_name("csv"),
        "A-dataset-processed-data-real.csv"
    );
}

#[test]
fn pseudo_2d_long_uses_the_actual_ruler_name_and_unit() {
    let spectrum = Arc::new(StackSpectrum {
        ppm: vec![7.0, 8.0],
        traces: vec![vec![Complex64::new(1.0, 2.0), Complex64::new(3.0, 4.0)]],
        direct: plotx_processing::AxisMeta {
            nucleus: "1H".into(),
            observe_freq_mhz: 400.0,
        },
        source: String::new(),
    });
    let mut value = snapshot(
        SnapshotData::Pseudo2D {
            spectrum,
            ruler_name: "Delay".into(),
            ruler_unit: "s".into(),
            ruler: vec![0.5],
        },
        DataExportContent::ProcessedData,
    );
    value.request.channel = IntensityChannel::Imaginary;
    value.request.shape = TableShape::Long;
    assert_eq!(
        value.to_text(Delimiter::Tab).unwrap(),
        "Delay (s)\tppm\tintensity\n0.5\t7\t2\n0.5\t8\t4\n"
    );
}

#[test]
fn electrophysiology_export_includes_only_selected_sweeps_and_pads_short_ones() {
    use plotx_io::{ElectricalUnit, ElectrophysiologyData, RecordedChannel, Sweep};

    let data = ElectrophysiologyData {
        abf_version: "2".into(),
        sample_rate_hz: 2.0,
        channels: vec![RecordedChannel {
            name: "Current".into(),
            unit: ElectricalUnit::from_symbol("pA"),
        }],
        sweeps: vec![
            Sweep {
                start_time_s: 0.0,
                channels: vec![vec![1.0, 2.0]],
                commands: Vec::new(),
            },
            Sweep {
                start_time_s: 0.0,
                channels: vec![vec![3.0]],
                commands: Vec::new(),
            },
        ],
        protocol: None,
        source: "recording.abf".into(),
        import_warnings: Vec::new(),
    };
    let mut recording = crate::state::ElectrophysiologyDataset::load(data);
    recording.processing.gaussian_lowpass_enabled = false;
    recording.selected_sweeps = vec![true, false];
    let dataset = Dataset::Electrophysiology(Box::new(recording));
    let value = DataExportSnapshot::capture(
        &dataset,
        DataExportRequest {
            content: DataExportContent::ProcessedData,
            channel: IntensityChannel::Real,
            shape: TableShape::Matrix,
        },
    )
    .unwrap();
    assert_eq!(
        value.to_text(Delimiter::Comma).unwrap(),
        "Time (s),Current (pA) — Sweep 1\n0,1\n0.5,2\n"
    );
}

#[test]
fn analysis_tables_keep_stable_headers_and_escape_user_text() {
    let peak = crate::state::ResolvedPeak {
        x: 1.25,
        y: 9.0,
        label: "a, \"peak\"".into(),
        origin: PeakOrigin::Manual,
        mark_id: Some(1),
    };
    let peaks = DataExportSnapshot {
        dataset_name: "sample, one".into(),
        request: request(DataExportContent::Peaks),
        data: SnapshotData::Peaks(vec![peak]),
    };
    assert_eq!(
        peaks.to_text(Delimiter::Comma).unwrap(),
        "dataset,x,y,origin,label\n\"sample, one\",1.25,9,manual,\"a, \"\"peak\"\"\"\n"
    );

    let integral = Integral2D {
        id: 1,
        name: "cross, peak".into(),
        f2: (1.0, 2.0),
        f1: (3.0, 4.0),
        volume: -5.0,
        normalized_volume: None,
        reference_value: Some(2.0),
        mode: crate::DisplayModeLabel::Real,
        method: IntegralMethod::Sum,
        baseline: BaselineMode::Plane,
    };
    let integrals = snapshot(
        SnapshotData::Integrals2D(vec![integral]),
        DataExportContent::Integrals,
    );
    let text = integrals.to_text(Delimiter::Comma).unwrap();
    assert!(text.starts_with(
        "name,f2_lo,f2_hi,f1_lo,f1_hi,volume,normalized_volume,reference_value,mode,method,baseline\n"
    ));
    assert!(text.ends_with("\"cross, peak\",1,2,3,4,-5,,2,real,sum,plane\n"));
}

#[test]
fn empty_dataset_returns_an_explicit_unavailable_error() {
    let dataset = Dataset::Table(Box::new(
        materialized_float_series_table(
            ("x".into(), "".into(), Vec::new()),
            Vec::new(),
            "plotx.test.empty-export-table.v1",
        )
        .unwrap(),
    ));
    assert!(DataExportAvailability::for_dataset(&dataset).is_empty());
    let error = DataExportSnapshot::capture(&dataset, request(DataExportContent::TypedTable))
        .err()
        .unwrap();
    assert!(matches!(error, DataExportError::Unavailable));
    assert_eq!(error.category(), "unavailable");
}

#[test]
fn default_channel_tracks_the_enabled_magnitude_display_step() {
    use plotx_io::{Domain, NmrData};
    use plotx_processing::{ProcessingStep, StepKind, StepSource};

    let data = NmrData {
        points: vec![Complex64::new(1.0, 2.0)],
        domain: Domain::Frequency,
        spectral_width_hz: 1_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 4.7,
        nucleus: "1H".into(),
        source: "spectrum".into(),
        group_delay: 0.0,
    };
    let mut nmr = crate::state::NmrDataset::load(data);
    let dataset = Dataset::Nmr(Box::new(nmr.clone()));
    assert_eq!(
        DataExportAvailability::for_dataset(&dataset).default_channel,
        IntensityChannel::Real
    );
    let id = nmr.allocate_step_id();
    nmr.pipeline.steps.push(ProcessingStep::new(
        id,
        StepKind::Magnitude,
        StepSource::User,
    ));
    let dataset = Dataset::Nmr(Box::new(nmr));
    assert_eq!(
        DataExportAvailability::for_dataset(&dataset).default_channel,
        IntensityChannel::Magnitude
    );
}
