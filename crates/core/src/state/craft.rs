use super::{FloatSeries, NmrDataset, TableDataset, materialized_float_series_table};
use plotx_io::NmrData;
use plotx_processing::craft::{
    CRAFT_ALGORITHM, CRAFT_ALGORITHM_VERSION, CraftComponent, CraftDiagnostics, CraftInvocation,
    CraftReference, CraftRegionRatio, CraftRegionSummary, CraftResult,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CraftRunId(pub u64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftProvenance {
    pub algorithm: String,
    pub version: u32,
    pub input_sha256: String,
    pub invocation: CraftInvocation,
    pub parent_run: Option<CraftRunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredCraftRun {
    pub id: CraftRunId,
    pub provenance: CraftProvenance,
    pub components: Vec<CraftComponent>,
    pub region_summaries: Vec<CraftRegionSummary>,
    pub region_ratio: Option<CraftRegionRatio>,
    pub diagnostics: CraftDiagnostics,
    /// Materialized full component data, when the user has requested it.
    pub component_table: Option<crate::state::DatasetId>,
}

impl StoredCraftRun {
    pub fn from_result(
        id: CraftRunId,
        data: &NmrData,
        invocation: CraftInvocation,
        parent_run: Option<CraftRunId>,
        result: CraftResult,
    ) -> Self {
        Self {
            id,
            provenance: CraftProvenance {
                algorithm: CRAFT_ALGORITHM.to_owned(),
                version: CRAFT_ALGORITHM_VERSION,
                input_sha256: craft_input_sha256(data),
                invocation,
                parent_run,
            },
            components: result.components,
            region_summaries: result.region_summaries,
            region_ratio: result.region_ratio,
            diagnostics: result.diagnostics,
            component_table: None,
        }
    }

    pub fn is_stale_for(&self, data: &NmrData, reference: CraftReference) -> bool {
        self.provenance.input_sha256 != craft_input_sha256(data)
            || self.provenance.invocation.reference != reference
    }
}

impl NmrDataset {
    /// Reference context used by analyses that fit the original FID but report
    /// chemical shifts on the processed spectrum's visible axis.
    pub fn craft_reference(&self) -> CraftReference {
        CraftReference::new(
            self.data.carrier_ppm,
            self.pipeline.chemical_shift_reference_offset_ppm(),
        )
    }

    pub fn allocate_craft_run_id(&mut self) -> CraftRunId {
        let id = CraftRunId(self.next_craft_run_id);
        self.next_craft_run_id = self
            .next_craft_run_id
            .checked_add(1)
            .expect("CRAFT run id overflow");
        id
    }

    pub fn repair_craft_run_allocator(&mut self) {
        let required = self
            .craft_runs
            .iter()
            .map(|run| run.id.0.saturating_add(1))
            .max()
            .unwrap_or(0);
        self.next_craft_run_id = self.next_craft_run_id.max(required);
    }

    pub fn craft_run(&self, id: CraftRunId) -> Option<&StoredCraftRun> {
        self.craft_runs.iter().find(|run| run.id == id)
    }

    pub fn store_craft_run(&mut self, run: StoredCraftRun) {
        self.craft_runs.push(run);
        self.clear_craft_spectrum_cache();
        self.reconcile_craft_fields();
    }
}

pub fn craft_input_sha256(data: &NmrData) -> String {
    let mut digest = Sha256::new();
    digest.update(b"plotx.craft.input.v1\0");
    digest.update([match data.domain {
        plotx_io::Domain::Time => 0,
        plotx_io::Domain::Frequency => 1,
    }]);
    for value in [
        data.spectral_width_hz,
        data.observe_freq_mhz,
        data.carrier_ppm,
        data.group_delay,
    ] {
        digest.update(value.to_le_bytes());
    }
    digest.update((data.points.len() as u64).to_le_bytes());
    for point in &data.points {
        digest.update(point.re.to_le_bytes());
        digest.update(point.im.to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}

pub fn craft_component_table(run: &StoredCraftRun) -> Result<TableDataset, String> {
    let rows = run.components.len();
    let column = |name: &str,
                  unit: &str,
                  values: Vec<Option<f64>>,
                  uncertainty: Option<Vec<Option<f64>>>| FloatSeries {
        name: name.to_owned(),
        unit: unit.to_owned(),
        values,
        uncertainty,
        fit: None,
    };
    let values = |read: fn(&CraftComponent) -> f64| {
        run.components
            .iter()
            .map(|component| Some(read(component)))
            .collect::<Vec<_>>()
    };
    let uncertainties = |read: fn(&CraftComponent) -> Option<f64>| {
        run.components.iter().map(read).collect::<Vec<_>>()
    };
    let region_numbers = run
        .components
        .iter()
        .map(|component| {
            run.region_summaries
                .iter()
                .position(|summary| summary.region == component.region)
                .map(|position| (position + 1) as f64)
        })
        .collect();
    let group_values = |read: fn(&plotx_processing::craft::CraftRegionSummary) -> f64| {
        run.components
            .iter()
            .map(|component| {
                run.region_summaries
                    .iter()
                    .find(|summary| summary.region == component.region)
                    .map(read)
            })
            .collect::<Vec<_>>()
    };
    materialized_float_series_table(
        (
            "component".into(),
            "".into(),
            (1..=rows).map(|row| Some(row as f64)).collect(),
        ),
        vec![
            column(
                "chemical shift",
                "ppm",
                values(|c| c.chemical_shift_ppm),
                None,
            ),
            column(
                "frequency",
                "Hz",
                values(|c| c.frequency_hz),
                Some(uncertainties(|c| c.frequency_std_hz)),
            ),
            column(
                "amplitude at t0",
                "",
                values(|c| c.amplitude_t0),
                Some(uncertainties(|c| c.amplitude_std)),
            ),
            column(
                "phase",
                "rad",
                values(|c| c.phase_rad),
                Some(uncertainties(|c| c.phase_std_rad)),
            ),
            column(
                "linewidth",
                "Hz",
                values(|c| c.linewidth_hz),
                Some(uncertainties(|c| c.linewidth_std_hz)),
            ),
            column("decay rate", "s^-1", values(|c| c.decay_rate_s_inv), None),
            column(
                "amplitude / noise",
                "",
                values(|c| c.amplitude_to_noise),
                None,
            ),
            column("signal group", "", region_numbers, None),
            column(
                "signal group start",
                "ppm",
                group_values(|summary| summary.start_ppm),
                None,
            ),
            column(
                "signal group end",
                "ppm",
                group_values(|summary| summary.end_ppm),
                None,
            ),
            column(
                "signal group coherent amplitude",
                "",
                group_values(|summary| summary.coherent_amplitude_t0),
                None,
            ),
            column(
                "run normalized residual",
                "",
                vec![Some(run.diagnostics.normalized_residual); rows],
                None,
            ),
        ],
        "plotx.analysis.craft-component-table.v1",
    )
    .map_err(|error| error.to_string())
}
