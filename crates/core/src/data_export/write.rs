//! Delimited-record serializers for each snapshot layout, plus the file-name
//! sanitizer used for suggested export names.

use super::{DataExportRequest, IntensityChannel, TableShape, XpsDataSnapshot, XpsFitParameterRow};
use crate::state::{PeakOrigin, ResolvedPeak, StoredCurveFitAnalysis};
use crate::{BaselineMode, Integral2D, IntegralMethod, IntegralResult};
use num_complex::Complex64;
use plotx_io::delimited::{DelimitedWriter, Field};
use plotx_processing::{Spectrum2D, StackSpectrum};
use std::io::{self, Write};

pub(super) fn safe_name(name: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for character in name.chars() {
        if character.is_alphanumeric() || matches!(character, '-' | '_') {
            result.push(character);
            separator = false;
        } else if !separator && !result.is_empty() {
            result.push('-');
            separator = true;
        }
    }
    let result = result.trim_matches('-');
    if result.is_empty() {
        "dataset".into()
    } else {
        result.into()
    }
}

pub(super) fn write_1d<W: Write>(
    writer: &mut DelimitedWriter<W>,
    axis: &[f64],
    axis_label: &str,
    values: &[Complex64],
    channel: IntensityChannel,
) -> io::Result<()> {
    writer.write_record(&[Field::Text(axis_label), Field::Text("intensity")])?;
    for index in 0..axis.len().max(values.len()) {
        writer.write_record(&[
            axis.get(index)
                .map_or(Field::Empty, |value| Field::Number(*value)),
            values
                .get(index)
                .map_or(Field::Empty, |value| Field::Number(channel.reduce(*value))),
        ])?;
    }
    Ok(())
}

pub(super) fn write_xrd<W: Write>(
    writer: &mut DelimitedWriter<W>,
    two_theta_deg: &[f64],
    intensity: &[f64],
) -> io::Result<()> {
    writer.write_record(&[Field::Text("two_theta_deg"), Field::Text("intensity")])?;
    for index in 0..two_theta_deg.len().max(intensity.len()) {
        writer.write_record(&[
            two_theta_deg
                .get(index)
                .map_or(Field::Empty, |value| Field::Number(*value)),
            intensity
                .get(index)
                .map_or(Field::Empty, |value| Field::Number(*value)),
        ])?;
    }
    Ok(())
}

pub(super) fn write_xps<W: Write>(
    writer: &mut DelimitedWriter<W>,
    data: &XpsDataSnapshot,
) -> io::Result<()> {
    let mut header = vec![
        Field::Text("native_energy_ev"),
        Field::Text("binding_energy_ev"),
        Field::Text("raw_cps"),
        Field::Text("processed_energy_ev"),
        Field::Text("processed_cps"),
        Field::Text("fit_energy_ev"),
        Field::Text("background_cps"),
        Field::Text("background_subtracted_cps"),
        Field::Text("envelope_cps"),
        Field::Text("residual_cps"),
        Field::Text("background_model"),
        Field::Text("fit_window_low_ev"),
        Field::Text("fit_window_high_ev"),
        Field::Text("low_anchor_low_ev"),
        Field::Text("low_anchor_high_ev"),
        Field::Text("high_anchor_low_ev"),
        Field::Text("high_anchor_high_ev"),
    ];
    header.extend(data.components.iter().map(|(label, _)| Field::Text(label)));
    writer.write_record(&header)?;
    let rows = data
        .native_energy_ev
        .len()
        .max(data.processed_energy_ev.len())
        .max(data.background.len());
    for row in 0..rows {
        let number = |values: &[f64]| values.get(row).copied().map_or(Field::Empty, Field::Number);
        let mut fields = vec![
            number(&data.native_energy_ev),
            data.binding_energy_ev
                .as_deref()
                .map_or(Field::Empty, &number),
            number(&data.raw_cps),
            number(&data.processed_energy_ev),
            number(&data.processed_cps),
            number(&data.fit_energy_ev),
            number(&data.background),
            number(&data.background_subtracted),
            number(&data.envelope),
            number(&data.residual),
            if row == 0 {
                data.background_model
                    .as_deref()
                    .map_or(Field::Empty, Field::Text)
            } else {
                Field::Empty
            },
            if row == 0 {
                data.background_window_ev
                    .map_or(Field::Empty, |value| Field::Number(value[0]))
            } else {
                Field::Empty
            },
            if row == 0 {
                data.background_window_ev
                    .map_or(Field::Empty, |value| Field::Number(value[1]))
            } else {
                Field::Empty
            },
            if row == 0 {
                data.low_anchor_ev
                    .map_or(Field::Empty, |value| Field::Number(value[0]))
            } else {
                Field::Empty
            },
            if row == 0 {
                data.low_anchor_ev
                    .map_or(Field::Empty, |value| Field::Number(value[1]))
            } else {
                Field::Empty
            },
            if row == 0 {
                data.high_anchor_ev
                    .map_or(Field::Empty, |value| Field::Number(value[0]))
            } else {
                Field::Empty
            },
            if row == 0 {
                data.high_anchor_ev
                    .map_or(Field::Empty, |value| Field::Number(value[1]))
            } else {
                Field::Empty
            },
        ];
        fields.extend(data.components.iter().map(|(_, values)| number(values)));
        writer.write_record(&fields)?;
    }
    Ok(())
}

pub(super) fn write_xps_fits<W: Write>(
    writer: &mut DelimitedWriter<W>,
    rows: &[XpsFitParameterRow],
) -> io::Result<()> {
    writer.write_record(&[
        Field::Text("provenance"),
        Field::Text("assignment"),
        Field::Text("position_ev"),
        Field::Text("fwhm_ev"),
        Field::Text("area"),
        Field::Text("area_fraction"),
        Field::Text("r_squared"),
        Field::Text("rmse"),
        Field::Text("residual_lag1"),
        Field::Text("position_at_bound"),
        Field::Text("fwhm_at_bound"),
        Field::Text("area_at_bound"),
        Field::Text("position_standard_error"),
        Field::Text("position_95_low_ev"),
        Field::Text("position_95_high_ev"),
        Field::Text("fwhm_standard_error"),
        Field::Text("fwhm_95_low_ev"),
        Field::Text("fwhm_95_high_ev"),
        Field::Text("area_standard_error"),
        Field::Text("area_95_low"),
        Field::Text("area_95_high"),
        Field::Text("maximum_correlation"),
        Field::Text("bootstrap_position_p2_5"),
        Field::Text("bootstrap_position_p50"),
        Field::Text("bootstrap_position_p97_5"),
        Field::Text("bootstrap_fwhm_p2_5"),
        Field::Text("bootstrap_fwhm_p50"),
        Field::Text("bootstrap_fwhm_p97_5"),
        Field::Text("bootstrap_area_p2_5"),
        Field::Text("bootstrap_area_p50"),
        Field::Text("bootstrap_area_p97_5"),
        Field::Text("bootstrap_fraction_p2_5"),
        Field::Text("bootstrap_fraction_p50"),
        Field::Text("bootstrap_fraction_p97_5"),
    ])?;
    for row in rows {
        let boolean = |value: Option<bool>| {
            value.map_or(Field::Empty, |value| {
                Field::Text(if value { "true" } else { "false" })
            })
        };
        writer.write_record(&[
            Field::Text(row.provenance),
            Field::Text(&row.label),
            Field::Number(row.center_ev),
            Field::Number(row.fwhm_ev),
            Field::Number(row.area),
            row.fraction.map_or(Field::Empty, Field::Number),
            row.r_squared.map_or(Field::Empty, Field::Number),
            row.rmse.map_or(Field::Empty, Field::Number),
            row.residual_lag1.map_or(Field::Empty, Field::Number),
            boolean(row.hit_position_bound),
            boolean(row.hit_fwhm_bound),
            boolean(row.hit_area_bound),
            row.center_standard_error
                .map_or(Field::Empty, Field::Number),
            interval(row.center_confidence_95, 0),
            interval(row.center_confidence_95, 1),
            row.fwhm_standard_error.map_or(Field::Empty, Field::Number),
            interval(row.fwhm_confidence_95, 0),
            interval(row.fwhm_confidence_95, 1),
            row.area_standard_error.map_or(Field::Empty, Field::Number),
            interval(row.area_confidence_95, 0),
            interval(row.area_confidence_95, 1),
            row.maximum_correlation.map_or(Field::Empty, Field::Number),
            interval(row.bootstrap_center, 0),
            interval(row.bootstrap_center, 1),
            interval(row.bootstrap_center, 2),
            interval(row.bootstrap_fwhm, 0),
            interval(row.bootstrap_fwhm, 1),
            interval(row.bootstrap_fwhm, 2),
            interval(row.bootstrap_area, 0),
            interval(row.bootstrap_area, 1),
            interval(row.bootstrap_area, 2),
            interval(row.bootstrap_fraction, 0),
            interval(row.bootstrap_fraction, 1),
            interval(row.bootstrap_fraction, 2),
        ])?;
    }
    Ok(())
}

fn interval<const N: usize>(value: Option<[f64; N]>, index: usize) -> Field<'static> {
    value.map_or(Field::Empty, |value| Field::Number(value[index]))
}

pub(super) fn write_true_2d<W: Write>(
    writer: &mut DelimitedWriter<W>,
    spectrum: &Spectrum2D,
    request: DataExportRequest,
) -> io::Result<()> {
    let f1_label = axis_column("f1", spectrum.indirect.unit);
    let f2_label = axis_column("f2", spectrum.direct.unit);
    if request.shape == TableShape::Long {
        writer.write_record(&[
            Field::Text(&f1_label),
            Field::Text(&f2_label),
            Field::Text("intensity"),
        ])?;
        for (row, f1) in spectrum.f1_ppm.iter().enumerate() {
            for (column, f2) in spectrum.f2_ppm.iter().enumerate() {
                let intensity = spectrum
                    .data
                    .get(row * spectrum.f2_size + column)
                    .copied()
                    .and_then(|value| match request.channel {
                        IntensityChannel::Magnitude => {
                            spectrum.magnitude_at(row * spectrum.f2_size + column)
                        }
                        _ => Some(request.channel.reduce(value)),
                    });
                writer.write_record(&[
                    Field::Number(*f1),
                    Field::Number(*f2),
                    intensity.map_or(Field::Empty, Field::Number),
                ])?;
            }
        }
        return Ok(());
    }
    let mut header = Vec::with_capacity(spectrum.f2_ppm.len() + 1);
    let corner = if spectrum.indirect.unit == Some(nmr::axis::AxisUnit::Ppm)
        && spectrum.direct.unit == Some(nmr::axis::AxisUnit::Ppm)
    {
        "F1/F2 (ppm)".to_owned()
    } else {
        format!("{f1_label}/{f2_label}")
    };
    header.push(Field::Text(&corner));
    header.extend(spectrum.f2_ppm.iter().map(|value| Field::Number(*value)));
    writer.write_record(&header)?;
    for (row, f1) in spectrum.f1_ppm.iter().enumerate() {
        let mut fields = Vec::with_capacity(spectrum.f2_ppm.len() + 1);
        fields.push(Field::Number(*f1));
        for column in 0..spectrum.f2_ppm.len() {
            fields.push(
                spectrum
                    .data
                    .get(row * spectrum.f2_size + column)
                    .copied()
                    .map_or(Field::Empty, |value| {
                        Field::Number(match request.channel {
                            IntensityChannel::Magnitude => spectrum
                                .magnitude_at(row * spectrum.f2_size + column)
                                .expect("view shape"),
                            _ => request.channel.reduce(value),
                        })
                    }),
            );
        }
        writer.write_record(&fields)?;
    }
    Ok(())
}

pub(super) fn write_pseudo_2d<W: Write>(
    writer: &mut DelimitedWriter<W>,
    spectrum: &StackSpectrum,
    ruler_name: &str,
    ruler_unit: &str,
    ruler: &[f64],
    request: DataExportRequest,
) -> io::Result<()> {
    let ruler_header = with_unit(ruler_name, ruler_unit);
    let direct_label = match spectrum.direct_domain {
        plotx_io::Domain::Time => "direct_time_s".to_owned(),
        plotx_io::Domain::Frequency => spectrum.direct.unit_label().to_owned(),
    };
    if request.shape == TableShape::Long {
        writer.write_record(&[
            Field::Text(&ruler_header),
            Field::Text(&direct_label),
            Field::Text("intensity"),
        ])?;
        for (row, trace) in spectrum.traces.iter().enumerate() {
            for (column, ppm) in spectrum.ppm.iter().enumerate() {
                writer.write_record(&[
                    ruler
                        .get(row)
                        .map_or(Field::Empty, |value| Field::Number(*value)),
                    Field::Number(*ppm),
                    trace.get(column).copied().map_or(Field::Empty, |value| {
                        Field::Number(request.channel.reduce(value))
                    }),
                ])?;
            }
        }
        return Ok(());
    }
    let corner = format!("{ruler_header} / {direct_label}");
    let mut header = Vec::with_capacity(spectrum.ppm.len() + 1);
    header.push(Field::Text(&corner));
    header.extend(spectrum.ppm.iter().map(|value| Field::Number(*value)));
    writer.write_record(&header)?;
    for (row, trace) in spectrum.traces.iter().enumerate() {
        let mut fields = Vec::with_capacity(spectrum.ppm.len() + 1);
        fields.push(
            ruler
                .get(row)
                .map_or(Field::Empty, |value| Field::Number(*value)),
        );
        fields.extend((0..spectrum.ppm.len()).map(|column| {
            trace.get(column).copied().map_or(Field::Empty, |value| {
                Field::Number(request.channel.reduce(value))
            })
        }));
        writer.write_record(&fields)?;
    }
    Ok(())
}

fn axis_column(axis: &str, unit: Option<nmr::axis::AxisUnit>) -> String {
    match unit {
        Some(nmr::axis::AxisUnit::Second) => format!("{axis}_time_s"),
        _ => format!("{axis}_{}", plotx_processing::axis_unit_label(unit)),
    }
}

pub(super) fn write_peaks<W: Write>(
    writer: &mut DelimitedWriter<W>,
    dataset: &str,
    peaks: &[ResolvedPeak],
) -> io::Result<()> {
    writer.write_record(&[
        Field::Text("dataset"),
        Field::Text("x"),
        Field::Text("y"),
        Field::Text("origin"),
        Field::Text("label"),
    ])?;
    for peak in peaks {
        let origin = match peak.origin {
            PeakOrigin::Detected => "detected",
            PeakOrigin::Manual => "manual",
        };
        writer.write_record(&[
            Field::Text(dataset),
            Field::Number(peak.x),
            Field::Number(peak.y),
            Field::Text(origin),
            Field::Text(&peak.label),
        ])?;
    }
    Ok(())
}

pub(super) fn write_integrals_1d<W: Write>(
    writer: &mut DelimitedWriter<W>,
    values: &[IntegralResult],
) -> io::Result<()> {
    writer.write_record(&[
        Field::Text("start_ppm"),
        Field::Text("end_ppm"),
        Field::Text("area"),
        Field::Text("normalized_area"),
        Field::Text("reference_value"),
        Field::Text("mode"),
    ])?;
    for value in values {
        writer.write_record(&[
            Field::Number(value.start_ppm),
            Field::Number(value.end_ppm),
            Field::Number(value.area),
            Field::Number(value.normalized_area),
            value
                .reference_value
                .map(Field::Number)
                .unwrap_or(Field::Empty),
            Field::Text(value.mode.as_str()),
        ])?;
    }
    Ok(())
}

pub(super) fn write_integrals_2d<W: Write>(
    writer: &mut DelimitedWriter<W>,
    values: &[Integral2D],
) -> io::Result<()> {
    writer.write_record(&[
        Field::Text("name"),
        Field::Text("f2_lo"),
        Field::Text("f2_hi"),
        Field::Text("f1_lo"),
        Field::Text("f1_hi"),
        Field::Text("volume"),
        Field::Text("normalized_volume"),
        Field::Text("reference_value"),
        Field::Text("mode"),
        Field::Text("method"),
        Field::Text("baseline"),
    ])?;
    for value in values {
        let normalized = value.normalized_volume.map_or(Field::Empty, Field::Number);
        let method = match value.method {
            IntegralMethod::Sum => "sum",
        };
        let baseline = match value.baseline {
            BaselineMode::None => "none",
            BaselineMode::Constant => "constant",
            BaselineMode::Plane => "plane",
        };
        writer.write_record(&[
            Field::Text(&value.name),
            Field::Number(value.f2.0),
            Field::Number(value.f2.1),
            Field::Number(value.f1.0),
            Field::Number(value.f1.1),
            Field::Number(value.volume),
            normalized,
            value.reference_value.map_or(Field::Empty, Field::Number),
            Field::Text(value.mode.as_str()),
            Field::Text(method),
            Field::Text(baseline),
        ])?;
    }
    Ok(())
}

pub(super) fn write_fits<W: Write>(
    writer: &mut DelimitedWriter<W>,
    analyses: &[StoredCurveFitAnalysis],
) -> io::Result<()> {
    writer.write_record(&[
        Field::Text("record_type"),
        Field::Text("analysis"),
        Field::Text("dataset"),
        Field::Text("response"),
        Field::Text("model"),
        Field::Text("name"),
        Field::Text("value"),
        Field::Text("standard_error"),
        Field::Text("row"),
        Field::Text("row_id"),
        Field::Text("observed"),
        Field::Text("predicted"),
        Field::Text("residual"),
        Field::Text("related_name"),
        Field::Text("details"),
    ])?;
    for analysis in analyses {
        let result = &analysis.result;
        for parameter in &result.parameters {
            write_fit_record(
                writer,
                "parameter",
                analysis,
                FitRecord {
                    dataset: parameter.dataset_id.as_deref(),
                    name: Some(&parameter.parameter),
                    value: Some(parameter.value),
                    standard_error: Some(parameter.standard_error),
                    ..FitRecord::default()
                },
            )?;
        }
        for derived in &result.derived {
            write_fit_record(
                writer,
                "derived",
                analysis,
                FitRecord {
                    dataset: Some(&derived.dataset_id),
                    name: Some(&derived.name),
                    value: Some(derived.value),
                    standard_error: Some(derived.standard_error),
                    ..FitRecord::default()
                },
            )?;
        }
        let global = [
            ("chi_squared", Some(result.statistics.chi_squared)),
            (
                "reduced_chi_squared",
                Some(result.statistics.reduced_chi_squared),
            ),
            ("r_squared", Some(result.statistics.r_squared)),
            ("aic", Some(result.statistics.aic)),
            ("bic", Some(result.statistics.bic)),
            ("aicc", result.statistics.aicc),
        ];
        for (name, value) in global {
            let Some(value) = value else { continue };
            write_fit_record(
                writer,
                "diagnostic",
                analysis,
                FitRecord {
                    name: Some(name),
                    value: Some(value),
                    ..FitRecord::default()
                },
            )?;
        }
        for statistic in &result.statistics.responses {
            for (name, value) in [
                ("chi_squared", statistic.chi_squared),
                ("reduced_chi_squared", statistic.reduced_chi_squared),
                ("r_squared", statistic.r_squared),
            ] {
                write_fit_record(
                    writer,
                    "diagnostic",
                    analysis,
                    FitRecord {
                        dataset: Some(&statistic.dataset_id),
                        response: Some(&statistic.response),
                        name: Some(name),
                        value: Some(value),
                        ..FitRecord::default()
                    },
                )?;
            }
        }
        write_fit_matrix(writer, "covariance", analysis, &result.covariance)?;
        write_fit_matrix(writer, "correlation", analysis, &result.correlation)?;
        for point in &result.points {
            let row_id = analysis
                .selection
                .as_ref()
                .and_then(|selection| {
                    selection
                        .instances
                        .iter()
                        .find(|instance| instance.dataset_id == point.dataset_id)
                        .and_then(|instance| instance.included_rows.get(point.row))
                })
                .map(ToString::to_string);
            write_fit_record(
                writer,
                "point",
                analysis,
                FitRecord {
                    dataset: Some(&point.dataset_id),
                    response: Some(&point.response),
                    row: Some(point.row as f64),
                    row_id: row_id.as_deref(),
                    observed: Some(point.observed),
                    predicted: Some(point.predicted),
                    residual: Some(point.residual),
                    ..FitRecord::default()
                },
            )?;
        }
        if let Some(selection) = &analysis.selection {
            for instance in &selection.instances {
                for exclusion in &instance.excluded_rows {
                    let quantities = exclusion.quantities.join("|");
                    write_fit_record(
                        writer,
                        "excluded_row",
                        analysis,
                        FitRecord {
                            dataset: Some(&instance.dataset_id),
                            name: Some(match exclusion.reason {
                                crate::state::FitRowExclusionReason::NullRequiredValue => {
                                    "null_required_value"
                                }
                                crate::state::FitRowExclusionReason::NonFiniteRequiredValue => {
                                    "non_finite_required_value"
                                }
                                crate::state::FitRowExclusionReason::NullAndNonFiniteRequiredValues => {
                                    "null_and_non_finite_required_values"
                                }
                            }),
                            row_id: Some(&exclusion.row.to_string()),
                            details: Some(&quantities),
                            ..FitRecord::default()
                        },
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// One long-form curve-fit export row; unset fields serialize as `Empty`.
/// The record type, analysis name, and model name are supplied per call.
#[derive(Default)]
struct FitRecord<'a> {
    dataset: Option<&'a str>,
    response: Option<&'a str>,
    name: Option<&'a str>,
    value: Option<f64>,
    standard_error: Option<f64>,
    row: Option<f64>,
    row_id: Option<&'a str>,
    observed: Option<f64>,
    predicted: Option<f64>,
    residual: Option<f64>,
    related_name: Option<&'a str>,
    details: Option<&'a str>,
}

fn write_fit_record<W: Write>(
    writer: &mut DelimitedWriter<W>,
    record_type: &str,
    analysis: &StoredCurveFitAnalysis,
    record: FitRecord,
) -> io::Result<()> {
    fn text(value: Option<&str>) -> Field<'_> {
        value.map_or(Field::Empty, Field::Text)
    }
    fn number<'a>(value: Option<f64>) -> Field<'a> {
        value.map_or(Field::Empty, Field::Number)
    }
    writer.write_record(&[
        Field::Text(record_type),
        Field::Text(&analysis.name),
        text(record.dataset),
        text(record.response),
        Field::Text(&analysis.result.model.name),
        text(record.name),
        number(record.value),
        number(record.standard_error),
        number(record.row),
        text(record.row_id),
        number(record.observed),
        number(record.predicted),
        number(record.residual),
        text(record.related_name),
        text(record.details),
    ])
}

/// A parameter-by-parameter matrix (covariance or correlation) as one row per
/// cell, named by the row parameter with the column parameter as related name.
fn write_fit_matrix<W: Write>(
    writer: &mut DelimitedWriter<W>,
    record_type: &str,
    analysis: &StoredCurveFitAnalysis,
    matrix: &[Vec<f64>],
) -> io::Result<()> {
    let parameter_name = |index: usize| {
        analysis
            .result
            .parameters
            .get(index)
            .map(|parameter| parameter.parameter.as_str())
            .unwrap_or("parameter")
    };
    for (row, values) in matrix.iter().enumerate() {
        for (column, value) in values.iter().enumerate() {
            write_fit_record(
                writer,
                record_type,
                analysis,
                FitRecord {
                    name: Some(parameter_name(row)),
                    value: Some(*value),
                    related_name: Some(parameter_name(column)),
                    ..FitRecord::default()
                },
            )?;
        }
    }
    Ok(())
}

pub(super) fn write_electrophysiology<W: Write>(
    writer: &mut DelimitedWriter<W>,
    sample_rate_hz: f64,
    channel_label: &str,
    traces: &[(usize, Vec<f64>)],
) -> io::Result<()> {
    let sweep_headers: Vec<String> = traces
        .iter()
        .map(|(sweep, _)| format!("{channel_label} — Sweep {sweep}"))
        .collect();
    let mut header = Vec::with_capacity(traces.len() + 1);
    header.push(Field::Text("Time (s)"));
    header.extend(sweep_headers.iter().map(|header| Field::Text(header)));
    writer.write_record(&header)?;
    let rows = traces
        .iter()
        .map(|(_, trace)| trace.len())
        .max()
        .unwrap_or(0);
    for row in 0..rows {
        let mut fields = Vec::with_capacity(traces.len() + 1);
        fields.push(Field::Number(row as f64 / sample_rate_hz));
        fields.extend(traces.iter().map(|(_, trace)| {
            trace
                .get(row)
                .map_or(Field::Empty, |value| Field::Number(*value))
        }));
        writer.write_record(&fields)?;
    }
    Ok(())
}

fn with_unit(label: &str, unit: &str) -> String {
    if unit.is_empty() {
        label.to_owned()
    } else {
        format!("{label} ({unit})")
    }
}
