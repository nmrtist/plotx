//! Conversions between processing pipelines/steps and their serialized DTOs.

use super::*;

pub fn pipeline_to_dto(pipe: &AxisPipeline) -> AxisPipelineDto {
    AxisPipelineDto {
        steps: pipe.steps.iter().map(step_to_dto).collect(),
    }
}

pub fn pipeline_from_dto(dto: &AxisPipelineDto) -> AxisPipeline {
    AxisPipeline {
        steps: dto.steps.iter().map(step_from_dto).collect(),
    }
}

/// Drop step identities from a pipeline destined for a detached recipe
/// (`.plotxproc`), which has no owner to make them meaningful.
pub fn strip_step_identities(dto: &mut AxisPipelineDto) {
    for step in &mut dto.steps {
        step.id = None;
    }
}

fn step_to_dto(step: &ProcessingStep) -> ProcessingStepDto {
    ProcessingStepDto {
        id: Some(step.id.get()),
        kind: kind_to_dto(&step.kind),
        enabled: step.enabled,
        source: source_to_dto(step.source),
    }
}

fn step_from_dto(dto: &ProcessingStepDto) -> ProcessingStep {
    // A recipe without identities decodes to placeholder numbering; every path
    // that hands such a pipeline to a dataset remints it (see `apply_scheme`).
    ProcessingStep {
        id: StepId::new(dto.id.unwrap_or(0)),
        kind: kind_from_dto(&dto.kind),
        enabled: dto.enabled,
        source: source_from_dto(dto.source),
    }
}

fn kind_to_dto(kind: &StepKind) -> StepKindDto {
    match kind {
        StepKind::Apodize(a) => StepKindDto::Apodize(apodization_to_dto(*a)),
        StepKind::ZeroFill(z) => StepKindDto::ZeroFill(zero_fill_to_str(*z)),
        StepKind::Fft => StepKindDto::Fft,
        StepKind::Phase(p) => StepKindDto::Phase(phase_to_dto(*p)),
        StepKind::Baseline(b) => StepKindDto::Baseline(baseline_to_dto(*b)),
        StepKind::Reference(r) => StepKindDto::Reference(ReferenceParamsDto {
            at_ppm: r.at_ppm,
            target_ppm: r.target_ppm,
        }),
        StepKind::Magnitude => StepKindDto::Magnitude,
        StepKind::Smooth(m) => StepKindDto::Smooth(smooth_to_dto(*m)),
        StepKind::Normalize(m) => StepKindDto::Normalize(normalize_to_dto(*m)),
        StepKind::Bin(p) => StepKindDto::Bin {
            width: p.width,
            method: bin_method_to_dto(p.method),
        },
        StepKind::Reverse => StepKindDto::Reverse,
        StepKind::Invert => StepKindDto::Invert,
    }
}

fn kind_from_dto(dto: &StepKindDto) -> StepKind {
    match dto {
        StepKindDto::Apodize(a) => StepKind::Apodize(apodization_from_dto(a)),
        StepKindDto::ZeroFill(z) => StepKind::ZeroFill(zero_fill_from_str(z)),
        StepKindDto::Fft => StepKind::Fft,
        StepKindDto::Phase(p) => StepKind::Phase(phase_from_dto(p)),
        StepKindDto::Baseline(b) => StepKind::Baseline(baseline_from_dto(b)),
        StepKindDto::Reference(r) => StepKind::Reference(ReferenceParams {
            at_ppm: r.at_ppm,
            target_ppm: r.target_ppm,
        }),
        StepKindDto::Magnitude => StepKind::Magnitude,
        StepKindDto::Smooth(m) => StepKind::Smooth(smooth_from_dto(*m)),
        StepKindDto::Normalize(m) => StepKind::Normalize(normalize_from_dto(*m)),
        StepKindDto::Bin { width, method } => StepKind::Bin(BinParams {
            width: *width,
            method: bin_method_from_dto(*method),
        }),
        StepKindDto::Reverse => StepKind::Reverse,
        StepKindDto::Invert => StepKind::Invert,
    }
}

fn smooth_to_dto(m: SmoothMethod) -> SmoothMethodDto {
    match m {
        SmoothMethod::MovingAverage { window } => SmoothMethodDto::MovingAverage { window },
        SmoothMethod::SavitzkyGolay { window, poly_order } => {
            SmoothMethodDto::SavitzkyGolay { window, poly_order }
        }
    }
}

fn smooth_from_dto(m: SmoothMethodDto) -> SmoothMethod {
    match m {
        SmoothMethodDto::MovingAverage { window } => SmoothMethod::MovingAverage { window },
        SmoothMethodDto::SavitzkyGolay { window, poly_order } => {
            SmoothMethod::SavitzkyGolay { window, poly_order }
        }
    }
}

fn normalize_to_dto(m: NormalizeMethod) -> NormalizeMethodDto {
    match m {
        NormalizeMethod::MaxPeak => NormalizeMethodDto::MaxPeak,
        NormalizeMethod::TotalArea => NormalizeMethodDto::TotalArea,
        NormalizeMethod::Constant { divisor } => NormalizeMethodDto::Constant { divisor },
    }
}

fn normalize_from_dto(m: NormalizeMethodDto) -> NormalizeMethod {
    match m {
        NormalizeMethodDto::MaxPeak => NormalizeMethod::MaxPeak,
        NormalizeMethodDto::TotalArea => NormalizeMethod::TotalArea,
        NormalizeMethodDto::Constant { divisor } => NormalizeMethod::Constant { divisor },
    }
}

fn bin_method_to_dto(m: BinMethod) -> BinMethodDto {
    match m {
        BinMethod::Sum => BinMethodDto::Sum,
        BinMethod::Mean => BinMethodDto::Mean,
    }
}

fn bin_method_from_dto(m: BinMethodDto) -> BinMethod {
    match m {
        BinMethodDto::Sum => BinMethod::Sum,
        BinMethodDto::Mean => BinMethod::Mean,
    }
}

fn apodization_to_dto(a: Apodization) -> ApodizationDto {
    match a {
        Apodization::None => ApodizationDto::None,
        Apodization::CosineBell => ApodizationDto::CosineBell,
        Apodization::Exponential { lb_hz } => ApodizationDto::Exponential { lb_hz },
        Apodization::Gaussian { lb_hz, gb_hz } => ApodizationDto::Gaussian { lb_hz, gb_hz },
    }
}

fn apodization_from_dto(a: &ApodizationDto) -> Apodization {
    match *a {
        ApodizationDto::None => Apodization::None,
        ApodizationDto::CosineBell => Apodization::CosineBell,
        ApodizationDto::Exponential { lb_hz } => Apodization::Exponential { lb_hz },
        ApodizationDto::Gaussian { lb_hz, gb_hz } => Apodization::Gaussian { lb_hz, gb_hz },
    }
}

fn phase_to_dto(p: PhaseParams) -> PhaseParamsDto {
    PhaseParamsDto {
        phase0: p.phase0,
        phase1: p.phase1,
        pivot_frac: p.pivot_frac,
        auto: p.auto.map(auto_phase_to_dto),
    }
}

fn phase_from_dto(p: &PhaseParamsDto) -> PhaseParams {
    PhaseParams {
        phase0: p.phase0,
        phase1: p.phase1,
        pivot_frac: p.pivot_frac,
        auto: p.auto.map(auto_phase_from_dto),
    }
}

fn baseline_to_dto(b: BaselineMethod) -> BaselineMethodDto {
    match b {
        BaselineMethod::Offset => BaselineMethodDto::Offset,
        BaselineMethod::Polynomial { order } => BaselineMethodDto::Polynomial { order },
        BaselineMethod::AsymmetricLeastSquares {
            smoothness,
            asymmetry,
            iterations,
        } => BaselineMethodDto::AsymmetricLeastSquares {
            smoothness,
            asymmetry,
            iterations,
        },
    }
}

fn baseline_from_dto(b: &BaselineMethodDto) -> BaselineMethod {
    match *b {
        BaselineMethodDto::Offset => BaselineMethod::Offset,
        BaselineMethodDto::Polynomial { order } => BaselineMethod::Polynomial { order },
        BaselineMethodDto::AsymmetricLeastSquares {
            smoothness,
            asymmetry,
            iterations,
        } => BaselineMethod::AsymmetricLeastSquares {
            smoothness,
            asymmetry,
            iterations,
        },
    }
}

fn source_to_dto(s: StepSource) -> StepSourceDto {
    match s {
        StepSource::Default => StepSourceDto::Default,
        StepSource::User => StepSourceDto::User,
        StepSource::Imported => StepSourceDto::Imported,
    }
}

fn source_from_dto(s: StepSourceDto) -> StepSource {
    match s {
        StepSourceDto::Default => StepSource::Default,
        StepSourceDto::User => StepSource::User,
        StepSourceDto::Imported => StepSource::Imported,
    }
}

fn zero_fill_to_str(zf: ZeroFill) -> String {
    match zf {
        ZeroFill::None => "none".to_owned(),
        ZeroFill::Factor(f) => format!("factor{f}"),
        ZeroFill::Size(s) => format!("size{s}"),
    }
}

fn zero_fill_from_str(s: &str) -> ZeroFill {
    if s == "none" {
        ZeroFill::None
    } else if let Some(rest) = s.strip_prefix("factor") {
        rest.parse().map(ZeroFill::Factor).unwrap_or(ZeroFill::None)
    } else if let Some(rest) = s.strip_prefix("size") {
        rest.parse().map(ZeroFill::Size).unwrap_or(ZeroFill::None)
    } else {
        ZeroFill::None
    }
}

fn auto_phase_to_dto(method: AutoPhaseMethod) -> AutoPhaseMethodDto {
    match method {
        AutoPhaseMethod::RobustConsensus => AutoPhaseMethodDto::RobustConsensus,
        AutoPhaseMethod::AbsorptivePeak => AutoPhaseMethodDto::AbsorptivePeak,
        AutoPhaseMethod::Entropy => AutoPhaseMethodDto::Entropy,
        AutoPhaseMethod::NegativeMinimization => AutoPhaseMethodDto::NegativeMinimization,
        AutoPhaseMethod::PeakRegression => AutoPhaseMethodDto::PeakRegression,
    }
}

fn auto_phase_from_dto(method: AutoPhaseMethodDto) -> AutoPhaseMethod {
    match method {
        AutoPhaseMethodDto::RobustConsensus => AutoPhaseMethod::RobustConsensus,
        AutoPhaseMethodDto::AbsorptivePeak => AutoPhaseMethod::AbsorptivePeak,
        AutoPhaseMethodDto::Entropy => AutoPhaseMethod::Entropy,
        AutoPhaseMethodDto::NegativeMinimization => AutoPhaseMethod::NegativeMinimization,
        AutoPhaseMethodDto::PeakRegression => AutoPhaseMethod::PeakRegression,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_correction_parameters_round_trip_through_dtos() {
        let phase = PhaseParams {
            auto: Some(AutoPhaseMethod::RobustConsensus),
            ..PhaseParams::MANUAL_ZERO
        };
        assert_eq!(phase_from_dto(&phase_to_dto(phase)), phase);
        let baseline = BaselineMethod::AsymmetricLeastSquares {
            smoothness: 2.5e7,
            asymmetry: 0.002,
            iterations: 35,
        };
        assert_eq!(baseline_from_dto(&baseline_to_dto(baseline)), baseline);
    }

    #[test]
    fn cleanup_steps_round_trip_through_dtos_and_json() {
        let kinds = [
            StepKind::Smooth(SmoothMethod::MovingAverage { window: 7 }),
            StepKind::Smooth(SmoothMethod::SavitzkyGolay {
                window: 11,
                poly_order: 4,
            }),
            StepKind::Normalize(NormalizeMethod::MaxPeak),
            StepKind::Normalize(NormalizeMethod::TotalArea),
            StepKind::Normalize(NormalizeMethod::Constant { divisor: 3.5 }),
            StepKind::Bin(BinParams {
                width: 0.02,
                method: BinMethod::Mean,
            }),
            StepKind::Reverse,
            StepKind::Invert,
        ];
        let pipe = AxisPipeline {
            steps: kinds
                .iter()
                .enumerate()
                .map(|(index, k)| {
                    ProcessingStep::new(StepId::new(index as u64), k.clone(), StepSource::User)
                })
                .collect(),
        };
        let dto = pipeline_to_dto(&pipe);
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: AxisPipelineDto = serde_json::from_str(&json).unwrap();
        let back = pipeline_from_dto(&parsed);
        for (a, b) in pipe.steps.iter().zip(&back.steps) {
            assert_eq!(a.kind, b.kind);
        }
    }

    #[test]
    fn pipeline_json_without_cleanup_variants_still_loads() {
        let json = r#"{"steps":[
            {"id":10,"kind":{"Apodize":{"Exponential":{"lb_hz":1.0}}},"enabled":true,"source":"User"},
            {"id":11,"kind":"Fft","enabled":true,"source":"Default"},
            {"id":12,"kind":{"Baseline":"Offset"},"enabled":true,"source":"User"},
            {"id":13,"kind":"Magnitude","enabled":false,"source":"Default"}
        ]}"#;
        let dto: AxisPipelineDto = serde_json::from_str(json).unwrap();
        let pipe = pipeline_from_dto(&dto);
        assert_eq!(pipe.steps.len(), 4);
        assert!(matches!(pipe.steps[2].kind, StepKind::Baseline(_)));
    }
}
