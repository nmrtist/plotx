use super::{CraftComponent, CraftComponentId, CraftRegionId};
use num_complex::Complex64;
use serde::{Deserialize, Serialize};

/// User-controlled definition of a derived CRAFT amplitude report.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftReportDefinition {
    pub threshold_an: f64,
    pub segment_width_hz: f64,
    /// Empty selects every region in the source run.
    #[serde(default)]
    pub regions: Vec<CraftRegionId>,
}

impl Default for CraftReportDefinition {
    fn default() -> Self {
        Self {
            threshold_an: 3.3,
            segment_width_hz: 1.0,
            regions: Vec::new(),
        }
    }
}

impl CraftReportDefinition {
    pub fn validate(&self) -> Result<(), CraftReportError> {
        if !self.threshold_an.is_finite() || self.threshold_an <= 0.0 {
            return Err(CraftReportError::InvalidThreshold);
        }
        if !self.segment_width_hz.is_finite() || self.segment_width_hz <= 0.0 {
            return Err(CraftReportError::InvalidWidth);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftReportSegment {
    pub center_hz: f64,
    pub start_hz: f64,
    pub end_hz: f64,
    pub component_ids: Vec<CraftComponentId>,
    pub component_count: usize,
    pub scalar_amplitude_sum_t0: f64,
    pub coherent_amplitude_t0: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftAmplitudeReport {
    pub schema_version: u32,
    pub definition: CraftReportDefinition,
    pub segments: Vec<CraftReportSegment>,
}

impl CraftAmplitudeReport {
    pub fn validate_against(&self, components: &[CraftComponent]) -> Result<(), CraftReportError> {
        self.definition.validate()?;
        let mut seen = std::collections::HashSet::new();
        for segment in &self.segments {
            for id in &segment.component_ids {
                if !seen.insert(*id) || !components.iter().any(|component| component.id == *id) {
                    return Err(CraftReportError::UnknownComponent);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CraftReportError {
    #[error("CRAFT report threshold must be finite and positive")]
    InvalidThreshold,
    #[error("CRAFT report segment width must be finite and positive")]
    InvalidWidth,
    #[error("CRAFT report references an unknown component")]
    UnknownComponent,
}

/// Build a report from the complete retained component list. Components are
/// sorted by frequency, and overlapping windows are merged without duplicate
/// membership, making the result independent of input ordering.
pub fn calculate_craft_report(
    components: &[CraftComponent],
    definition: CraftReportDefinition,
) -> Result<CraftAmplitudeReport, CraftReportError> {
    definition.validate()?;
    let mut selected: Vec<&CraftComponent> = components
        .iter()
        .filter(|component| {
            (definition.regions.is_empty() || definition.regions.contains(&component.region))
                && component.amplitude_to_noise >= definition.threshold_an
        })
        .collect();
    selected.sort_by(|a, b| {
        a.frequency_hz
            .total_cmp(&b.frequency_hz)
            .then(a.id.0.cmp(&b.id.0))
    });
    let half = definition.segment_width_hz * 0.5;
    let mut segments = Vec::new();
    for component in selected {
        let start = component.frequency_hz - half;
        let end = component.frequency_hz + half;
        let append = segments
            .last()
            .is_none_or(|segment: &CraftReportSegment| start > segment.end_hz);
        if append {
            segments.push(CraftReportSegment {
                center_hz: component.frequency_hz,
                start_hz: start,
                end_hz: end,
                component_ids: vec![component.id],
                component_count: 1,
                scalar_amplitude_sum_t0: component.amplitude_t0,
                coherent_amplitude_t0: Complex64::from_polar(
                    component.amplitude_t0,
                    component.phase_rad,
                )
                .norm(),
            });
        } else {
            let segment = segments.last_mut().expect("segment exists");
            segment.end_hz = segment.end_hz.max(end);
            segment.center_hz = (segment.start_hz + segment.end_hz) * 0.5;
            if !segment.component_ids.contains(&component.id) {
                segment.component_ids.push(component.id);
                segment.component_count += 1;
                segment.scalar_amplitude_sum_t0 += component.amplitude_t0;
                let phase_sum = segment
                    .component_ids
                    .iter()
                    .filter_map(|id| components.iter().find(|candidate| candidate.id == *id))
                    .fold(Complex64::new(0.0, 0.0), |sum, item| {
                        sum + Complex64::from_polar(item.amplitude_t0, item.phase_rad)
                    });
                segment.coherent_amplitude_t0 = phase_sum.norm();
            }
        }
    }
    Ok(CraftAmplitudeReport {
        schema_version: 1,
        definition,
        segments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn component(
        id: u64,
        frequency_hz: f64,
        amplitude_to_noise: f64,
        phase_rad: f64,
    ) -> CraftComponent {
        CraftComponent {
            id: CraftComponentId(id),
            region: CraftRegionId(1),
            frequency_hz,
            chemical_shift_ppm: 0.0,
            amplitude_t0: 1.0,
            phase_rad,
            decay_rate_s_inv: 1.0,
            linewidth_hz: 1.0,
            amplitude_to_noise,
            amplitude_std: None,
            frequency_std_hz: None,
            linewidth_std_hz: None,
            phase_std_rad: None,
        }
    }
    #[test]
    fn filters_at_threshold_and_merges_without_duplicates() {
        let components = vec![
            component(2, 1.4, 3.3, std::f64::consts::PI),
            component(1, 1.0, 3.2, 0.0),
            component(3, 1.8, 4.0, 0.0),
        ];
        let report = calculate_craft_report(
            &components,
            CraftReportDefinition {
                threshold_an: 3.3,
                segment_width_hz: 1.0,
                regions: vec![],
            },
        )
        .unwrap();
        assert_eq!(report.segments.len(), 1);
        assert_eq!(
            report.segments[0].component_ids,
            vec![CraftComponentId(2), CraftComponentId(3)]
        );
        assert_eq!(report.segments[0].scalar_amplitude_sum_t0, 2.0);
        assert!((report.segments[0].coherent_amplitude_t0 - 0.0).abs() < 1e-12);
    }
    #[test]
    fn rejects_invalid_definition() {
        assert_eq!(
            calculate_craft_report(
                &[],
                CraftReportDefinition {
                    threshold_an: f64::NAN,
                    ..Default::default()
                }
            )
            .unwrap_err(),
            CraftReportError::InvalidThreshold
        );
        assert_eq!(
            calculate_craft_report(
                &[],
                CraftReportDefinition {
                    segment_width_hz: 0.0,
                    ..Default::default()
                }
            )
            .unwrap_err(),
            CraftReportError::InvalidWidth
        );
    }
}
