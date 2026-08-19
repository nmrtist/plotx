use super::*;
impl Dataset {
    pub fn scientific_identity(&self) -> &plotx_io::ImportedScientificIdentity {
        match self {
            Dataset::Nmr(data) => &data.scientific_identity,
            Dataset::Nmr2D(data) => &data.scientific_identity,
            Dataset::Table(data) => &data.scientific_identity,
            Dataset::Electrophysiology(data) => &data.scientific_identity,
            Dataset::Afm(data) => &data.scientific_identity,
            Dataset::MassSpec(data) => &data.scientific_identity,
            Dataset::Xrd(data) => &data.scientific_identity,
            Dataset::Xps(data) => &data.scientific_identity,
        }
    }

    pub fn set_scientific_identity(&mut self, identity: plotx_io::ImportedScientificIdentity) {
        match self {
            Dataset::Nmr(data) => data.scientific_identity = identity,
            Dataset::Nmr2D(data) => data.scientific_identity = identity,
            Dataset::Table(data) => data.scientific_identity = identity,
            Dataset::Electrophysiology(data) => data.scientific_identity = identity,
            Dataset::Afm(data) => data.scientific_identity = identity,
            Dataset::MassSpec(data) => data.scientific_identity = identity,
            Dataset::Xrd(data) => data.scientific_identity = identity,
            Dataset::Xps(data) => data.scientific_identity = identity,
        }
    }

    pub fn as_xps(&self) -> Option<&XpsDataset> {
        match self {
            Dataset::Xps(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_xps_mut(&mut self) -> Option<&mut XpsDataset> {
        match self {
            Dataset::Xps(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_afm(&self) -> Option<&AfmDataset> {
        match self {
            Dataset::Afm(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_afm_mut(&mut self) -> Option<&mut AfmDataset> {
        match self {
            Dataset::Afm(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_mass_spec(&self) -> Option<&MassSpecDataset> {
        match self {
            Dataset::MassSpec(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_mass_spec_mut(&mut self) -> Option<&mut MassSpecDataset> {
        match self {
            Dataset::MassSpec(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_xrd(&self) -> Option<&XrdDataset> {
        match self {
            Dataset::Xrd(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_xrd_mut(&mut self) -> Option<&mut XrdDataset> {
        match self {
            Dataset::Xrd(data) => Some(data),
            _ => None,
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            Dataset::Nmr(_) => "NMR 1D",
            Dataset::Nmr2D(_) => "NMR 2D",
            Dataset::Table(_) => "Data Table",
            Dataset::Electrophysiology(_) => "Electrophysiology",
            Dataset::Afm(_) => "AFM",
            Dataset::MassSpec(_) => "LC–MS",
            Dataset::Xrd(_) => "XRD",
            Dataset::Xps(_) => "XPS",
        }
    }

    /// The chart/tool domain this dataset belongs to — a stable key the chart
    /// registry dispatches on (see `state::charts`). A pseudo-2D array (a stack
    /// with a recovered ruler) is its own domain, distinct from a true-2D contour.
    pub fn domain(&self) -> DataDomain {
        match self {
            Dataset::Nmr(_) => DataDomain::Nmr1d,
            Dataset::Nmr2D(n) if n.is_pseudo() => DataDomain::PseudoNmr,
            Dataset::Nmr2D(_) => DataDomain::Nmr2d,
            Dataset::Table(_) => DataDomain::Table,
            Dataset::Electrophysiology(_) => DataDomain::Electrophysiology,
            Dataset::Afm(_) => DataDomain::Afm,
            Dataset::MassSpec(_) => DataDomain::MassSpectrometry,
            Dataset::Xrd(_) => DataDomain::Xrd,
            Dataset::Xps(_) => DataDomain::Xps,
        }
    }

    /// The user-facing label in the Data list: the custom name if one was set
    /// via rename, otherwise the derived `[kind] summary`.
    pub fn display_name(&self) -> String {
        let custom = match self {
            Dataset::Nmr(d) => d.name.clone(),
            Dataset::Nmr2D(d) => d.name.clone(),
            Dataset::Table(d) => d.name.clone(),
            Dataset::Electrophysiology(d) => d.name.clone(),
            Dataset::Afm(d) => d.name.clone(),
            Dataset::MassSpec(d) => d.name.clone(),
            Dataset::Xrd(d) => d.name.clone(),
            Dataset::Xps(d) => d.name.clone(),
        };
        custom.unwrap_or_else(|| format!("[{}] {}", self.kind_label(), self.summary()))
    }

    pub fn set_name(&mut self, name: Option<String>) {
        match self {
            Dataset::Nmr(d) => d.name = name,
            Dataset::Nmr2D(d) => d.name = name,
            Dataset::Table(d) => d.name = name,
            Dataset::Electrophysiology(d) => d.name = name,
            Dataset::Afm(d) => d.name = name,
            Dataset::MassSpec(d) => d.name = name,
            Dataset::Xrd(d) => d.name = name,
            Dataset::Xps(d) => d.name = name,
        }
    }

    pub fn name(&self) -> Option<String> {
        match self {
            Dataset::Nmr(d) => d.name.clone(),
            Dataset::Nmr2D(d) => d.name.clone(),
            Dataset::Table(d) => d.name.clone(),
            Dataset::Electrophysiology(d) => d.name.clone(),
            Dataset::Afm(d) => d.name.clone(),
            Dataset::MassSpec(d) => d.name.clone(),
            Dataset::Xrd(d) => d.name.clone(),
            Dataset::Xps(d) => d.name.clone(),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Dataset::Nmr(d) => format!(
                "{} · {} pts · {:.2} MHz",
                d.data.nucleus,
                d.data.len(),
                d.data.observe_freq_mhz
            ),
            Dataset::Nmr2D(d) => d.summary(),
            Dataset::Table(d) => d.summary(),
            Dataset::Electrophysiology(d) => format!(
                "{} channels · {} sweeps · {:.3} kHz",
                d.data.channels.len(),
                d.data.sweeps.len(),
                d.data.sample_rate_hz / 1_000.0
            ),
            Dataset::Afm(d) => {
                let curves = d
                    .data
                    .forces
                    .as_ref()
                    .map_or(0, |f| f.grid_width * f.grid_height);
                format!("{} channels · {curves} force curves", d.data.images.len())
            }
            Dataset::MassSpec(d) => format!(
                "{} MS streams · {} scans · {} detector channels",
                d.supported_ms_streams().count(),
                d.run
                    .streams
                    .iter()
                    .map(|stream| stream.spectra.len())
                    .sum::<usize>(),
                d.run.chromatograms.len()
            ),
            Dataset::Xrd(d) => format!(
                "{} points · {:.2}–{:.2} deg 2theta",
                d.data.len(),
                d.data.two_theta_deg.first().copied().unwrap_or(0.0),
                d.data.two_theta_deg.last().copied().unwrap_or(0.0)
            ),
            Dataset::Xps(d) => format!(
                "{} locations · {} regions",
                d.experiment.measurements.len(),
                d.experiment.regions.len()
            ),
        }
    }

    pub fn as_nmr_mut(&mut self) -> Option<&mut NmrDataset> {
        match self {
            Dataset::Nmr(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_nmr(&self) -> Option<&NmrDataset> {
        match self {
            Dataset::Nmr(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_nmr2d_mut(&mut self) -> Option<&mut Nmr2DDataset> {
        match self {
            Dataset::Nmr2D(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_nmr2d(&self) -> Option<&Nmr2DDataset> {
        match self {
            Dataset::Nmr2D(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_table_mut(&mut self) -> Option<&mut TableDataset> {
        match self {
            Dataset::Table(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<&TableDataset> {
        match self {
            Dataset::Table(d) => Some(d),
            _ => None,
        }
    }

    /// The dataset's peak set, for domains that carry one (1D spectra and tables).
    pub fn peaks(&self) -> Option<&PeakSet> {
        match self {
            Dataset::Nmr(d) => Some(&d.peaks),
            Dataset::Table(d) => Some(&d.peaks),
            Dataset::Nmr2D(_) => None,
            Dataset::Electrophysiology(_) => None,
            Dataset::Afm(_) => None,
            Dataset::MassSpec(_) => None,
            Dataset::Xrd(_) => None,
            Dataset::Xps(_) => None,
        }
    }

    pub fn peaks_mut(&mut self) -> Option<&mut PeakSet> {
        match self {
            Dataset::Nmr(d) => Some(&mut d.peaks),
            Dataset::Table(d) => Some(&mut d.peaks),
            Dataset::Nmr2D(_) => None,
            Dataset::Electrophysiology(_) => None,
            Dataset::Afm(_) => None,
            Dataset::MassSpec(_) => None,
            Dataset::Xrd(_) => None,
            Dataset::Xps(_) => None,
        }
    }

    /// The dataset's stored lineshape deconvolutions, for domains with a 1D trace.
    pub fn line_fits(&self) -> &[StoredLineFit] {
        match self {
            Dataset::Nmr(d) => &d.line_fits,
            Dataset::Table(d) => &d.line_fits,
            Dataset::Nmr2D(_) => &[],
            Dataset::Electrophysiology(_) => &[],
            Dataset::Afm(_) => &[],
            Dataset::MassSpec(_) => &[],
            Dataset::Xrd(_) => &[],
            Dataset::Xps(_) => &[],
        }
    }

    pub fn line_fits_mut(&mut self) -> Option<&mut Vec<StoredLineFit>> {
        match self {
            Dataset::Nmr(d) => Some(&mut d.line_fits),
            Dataset::Table(d) => Some(&mut d.line_fits),
            Dataset::Nmr2D(_) => None,
            Dataset::Electrophysiology(_) => None,
            Dataset::Afm(_) => None,
            Dataset::MassSpec(_) => None,
            Dataset::Xrd(_) => None,
            Dataset::Xps(_) => None,
        }
    }

    pub fn next_line_fit_id_mut(&mut self) -> Option<&mut u64> {
        match self {
            Dataset::Nmr(d) => Some(&mut d.next_line_fit_id),
            Dataset::Table(d) => Some(&mut d.next_line_fit_id),
            Dataset::Nmr2D(_) => None,
            Dataset::Electrophysiology(_) => None,
            Dataset::Afm(_) => None,
            Dataset::MassSpec(_) => None,
            Dataset::Xrd(_) => None,
            Dataset::Xps(_) => None,
        }
    }

    /// The dataset's stored multiplet analyses (1D NMR only: J values need an
    /// observe frequency to convert to Hz).
    pub fn multiplets(&self) -> &[StoredMultiplet] {
        match self {
            Dataset::Nmr(d) => &d.multiplets,
            _ => &[],
        }
    }

    pub fn multiplets_mut(&mut self) -> Option<&mut Vec<StoredMultiplet>> {
        match self {
            Dataset::Nmr(d) => Some(&mut d.multiplets),
            _ => None,
        }
    }

    pub fn next_multiplet_id_mut(&mut self) -> Option<&mut u64> {
        match self {
            Dataset::Nmr(d) => Some(&mut d.next_multiplet_id),
            _ => None,
        }
    }

    pub fn supports_region_analysis(&self) -> bool {
        self.field_descriptors().iter().any(|field| {
            field
                .capabilities
                .contains(crate::automation::CAP_FIELD_REGION_SERIES)
        }) && match self {
            Dataset::Nmr2D(dataset) => {
                dataset.is_pseudo()
                    && matches!(
                        &dataset.processed,
                        Processed2D::Stack(stack)
                            if stack.direct_domain == plotx_io::Domain::Frequency
                    )
            }
            Dataset::Electrophysiology(dataset) => {
                !dataset.data.channels.is_empty() && !dataset.data.sweeps.is_empty()
            }
            Dataset::Nmr(_)
            | Dataset::Table(_)
            | Dataset::Afm(_)
            | Dataset::MassSpec(_)
            | Dataset::Xrd(_) => false,
            Dataset::Xps(_) => false,
        }
    }

    pub fn region_analysis(&self) -> Option<&RegionAnalysisState> {
        match self {
            Dataset::Nmr2D(dataset) if self.supports_region_analysis() => {
                Some(&dataset.region_analysis)
            }
            Dataset::Electrophysiology(dataset) if self.supports_region_analysis() => {
                Some(&dataset.region_analysis)
            }
            _ => None,
        }
    }

    pub fn region_analysis_mut(&mut self) -> Option<&mut RegionAnalysisState> {
        let supported = self.supports_region_analysis();
        match self {
            Dataset::Nmr2D(dataset) if supported => Some(&mut dataset.region_analysis),
            Dataset::Electrophysiology(dataset) if supported => Some(&mut dataset.region_analysis),
            _ => None,
        }
    }

    pub fn region_axis_unit(&self) -> Option<&'static str> {
        match self {
            Dataset::Nmr2D(_) if self.supports_region_analysis() => Some("ppm"),
            Dataset::Electrophysiology(_) if self.supports_region_analysis() => Some("s"),
            _ => None,
        }
    }

    pub fn region_source_field(&self) -> Option<FieldId> {
        match self {
            Dataset::Nmr2D(_) if self.supports_region_analysis() => self.default_field_id(),
            Dataset::Electrophysiology(recording) if self.supports_region_analysis() => self
                .field_descriptors()
                .get(recording.selected_channel)
                .map(|field| field.id),
            _ => None,
        }
    }

    pub fn tool_groups(&self) -> &'static [ToolGroup] {
        match self {
            Dataset::Nmr(dataset) if dataset.output_domain() == plotx_io::Domain::Frequency => &[
                ToolGroup::Processing,
                ToolGroup::Nmr1dAnalysis,
                ToolGroup::Peaks,
                ToolGroup::LineFit,
            ],
            Dataset::Nmr(_) => &[ToolGroup::Processing],
            Dataset::Nmr2D(_) if self.supports_region_analysis() => &[
                ToolGroup::Processing,
                ToolGroup::Nmr2dExperiment,
                ToolGroup::RegionAnalysis,
            ],
            Dataset::Nmr2D(_) => &[ToolGroup::Processing, ToolGroup::Nmr2dExperiment],
            Dataset::Table(_) => &[
                ToolGroup::Peaks,
                ToolGroup::CurveFit,
                ToolGroup::LineFit,
                ToolGroup::Statistics,
            ],
            Dataset::Electrophysiology(_) if self.supports_region_analysis() => {
                &[ToolGroup::Electrophysiology, ToolGroup::RegionAnalysis]
            }
            Dataset::Electrophysiology(_) => &[ToolGroup::Electrophysiology],
            Dataset::Afm(_) => &[],
            Dataset::MassSpec(_) => &[ToolGroup::MassSpectrometry],
            Dataset::Xrd(_) => &[ToolGroup::Processing],
            Dataset::Xps(_) => &[ToolGroup::Xps],
        }
    }

    /// The phaseable/processable axes for this dataset: 1D and a stack expose the
    /// direct axis only; a true-2D spectrum exposes both F2 and F1; a table has
    /// no frequency axis to phase.
    pub fn phase_axes(&self) -> &'static [PhaseAxis] {
        match self {
            Dataset::Nmr(_) => &[PhaseAxis::Direct],
            Dataset::Nmr2D(n) if n.is_true_2d() => &[PhaseAxis::F2, PhaseAxis::F1],
            Dataset::Nmr2D(_) => &[PhaseAxis::F2],
            Dataset::Table(_) => &[],
            Dataset::Electrophysiology(_) => &[],
            Dataset::Afm(_) => &[],
            Dataset::MassSpec(_) => &[],
            Dataset::Xrd(_) => &[],
            Dataset::Xps(_) => &[],
        }
    }

    pub fn active_phase_axis(&self, requested: PhaseAxis) -> PhaseAxis {
        let axes = self.phase_axes();
        if axes.contains(&requested) {
            requested
        } else {
            *axes.first().unwrap_or(&PhaseAxis::Direct)
        }
    }

    pub fn axis_pipeline_mut(&mut self, axis: PhaseAxis) -> Option<&mut AxisPipeline> {
        match self {
            Dataset::Nmr(n) if axis == PhaseAxis::Direct => Some(n.pipeline_mut()),
            Dataset::Nmr2D(n) => n.axis_mut(axis),
            _ => None,
        }
    }

    pub fn axis_pipeline(&self, axis: PhaseAxis) -> Option<&AxisPipeline> {
        match self {
            Dataset::Nmr(n) if axis == PhaseAxis::Direct => Some(n.pipeline()),
            Dataset::Nmr2D(n) => match (axis, &n.processed) {
                (PhaseAxis::F2, _) => Some(&n.params.f2),
                (PhaseAxis::F1, Processed2D::Ft(_)) => Some(&n.params.f1),
                _ => None,
            },
            _ => None,
        }
    }

    /// The recipe this dataset would be loaded with today, for one axis.
    ///
    /// Produced by re-running the very factory that built the live one, so "what
    /// does reset give me" and "what does a new document get" stay one answer
    /// rather than two derivations that agree only until one of them changes.
    pub fn factory_pipeline(&self, axis: PhaseAxis) -> Option<AxisPipeline> {
        match self {
            Dataset::Nmr(n) if axis == PhaseAxis::Direct => Some(match n.data.domain {
                Domain::Time => AxisPipeline::default_1d(),
                Domain::Frequency => AxisPipeline::frequency_1d(),
            }),
            Dataset::Nmr2D(n) => {
                let params = match n.data.domain {
                    Domain::Time => Params2D::default_for(n.preset),
                    Domain::Frequency => Params2D::frequency_domain(n.preset),
                };
                Some(match axis {
                    PhaseAxis::F1 => params.f1,
                    _ => params.f2,
                })
            }
            _ => None,
        }
    }

    pub fn phase_params_mut(&mut self, axis: PhaseAxis) -> Option<&mut PhaseParams> {
        self.axis_pipeline_mut(axis).and_then(|pipe| {
            pipe.steps
                .iter_mut()
                .filter(|s| s.enabled)
                .find_map(|s| match &mut s.kind {
                    StepKind::Phase(p) => Some(p),
                    _ => None,
                })
        })
    }

    /// Parameters produced by the currently enabled automatic Phase step.
    /// This mirrors the processing kernels so switching to manual is lossless.
    pub fn automatic_phase_params(&self, axis: PhaseAxis) -> Option<(f64, f64, f64)> {
        let pipe = self.axis_pipeline(axis)?;
        let method =
            pipe.steps
                .iter()
                .filter(|step| step.enabled)
                .find_map(|step| match &step.kind {
                    StepKind::Phase(params) => params.auto,
                    _ => None,
                })?;
        match self {
            Dataset::Nmr(n) => Some(plotx_processing::auto_phase(n.base.as_frequency()?, method)),
            Dataset::Nmr2D(n) => match &n.base {
                Processed2D::Ft(s) => {
                    let peak_arg = s
                        .data
                        .iter()
                        .max_by(|a, b| a.norm().total_cmp(&b.norm()))
                        .map_or(0.0, |value| value.arg());
                    let (f2, f1) = s.peak_pivot_fracs();
                    Some((peak_arg, 0.0, if axis == PhaseAxis::F1 { f1 } else { f2 }))
                }
                Processed2D::Stack(s) if axis == PhaseAxis::F2 => {
                    let (phase0, phase1) =
                        plotx_processing::fft2::absorptive_phase(&s.traces).unwrap_or((0.0, 0.0));
                    Some((phase0, phase1, s.peak_pivot_frac()))
                }
                Processed2D::Stack(_) => None,
            },
            _ => None,
        }
    }

    pub fn pivot_ppm(&self, axis: PhaseAxis) -> Option<f64> {
        match self {
            Dataset::Nmr(n)
                if axis == PhaseAxis::Direct
                    && n.output_domain() == plotx_io::Domain::Frequency =>
            {
                Some(n.pivot_ppm())
            }
            Dataset::Nmr2D(n) => n.pivot_ppm(axis),
            _ => None,
        }
    }

    pub fn set_pivot_ppm(&mut self, axis: PhaseAxis, ppm: f64) {
        match self {
            Dataset::Nmr(n) if axis == PhaseAxis::Direct => n.set_pivot_ppm(ppm),
            Dataset::Nmr2D(n) => n.set_pivot_ppm(axis, ppm),
            _ => {}
        }
    }

    /// Re-express the same manual phase curve around a new ppm pivot.
    pub fn repivot_ppm(&mut self, axis: PhaseAxis, ppm: f64) -> bool {
        let Some((old_pivot, is_manual)) = self
            .phase_params_mut(axis)
            .map(|params| (params.pivot_frac, params.auto.is_none()))
        else {
            return false;
        };
        if !is_manual {
            return false;
        }
        self.set_pivot_ppm(axis, ppm);
        let Some(params) = self.phase_params_mut(axis) else {
            return false;
        };
        let new_pivot = params.pivot_frac;
        params.pivot_frac = old_pivot;
        params.repivot(new_pivot);
        new_pivot != old_pivot
    }
}
