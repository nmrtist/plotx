use super::{CraftRunId, DatasetId, Document};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReportId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReportKindId(pub String);

impl ReportKindId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReportSource {
    pub dataset: DatasetId,
    pub craft_run: CraftRunId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportStatus {
    Available,
    NeedsReview,
    Unavailable,
}

impl AnalysisReportRecord {
    pub fn status(&self, document: &Document) -> ReportStatus {
        let Some(dataset) = document
            .datasets
            .iter()
            .find(|d| d.resource_id() == self.source.dataset)
        else {
            return ReportStatus::Unavailable;
        };
        let Some(nmr) = dataset.as_nmr() else {
            return ReportStatus::Unavailable;
        };
        let Some(run) = nmr.craft_run(self.source.craft_run) else {
            return ReportStatus::Unavailable;
        };
        if self.kind.0 == "craft_amplitude"
            && (run.diagnostics.status != plotx_processing::craft::CraftRunStatus::Complete
                || !run.diagnostics.stability.passed)
        {
            return ReportStatus::NeedsReview;
        }
        if self.kind.0 == "craft_amplitude" {
            let valid_definition = serde_json::from_value::<
                plotx_processing::craft::CraftReportDefinition,
            >(self.definition.clone())
            .ok()
            .is_some_and(|definition| definition.validate().is_ok());
            if !valid_definition {
                return ReportStatus::NeedsReview;
            }
        }
        if self.kind.0 == "craft_amplitude"
            && serde_json::from_value::<plotx_processing::craft::CraftAmplitudeReport>(
                self.snapshot.clone(),
            )
            .ok()
            .is_none_or(|snapshot| snapshot.validate_against(&run.components).is_err())
        {
            return ReportStatus::NeedsReview;
        }
        if run.provenance.input_sha256 != self.source_fingerprint
            || run.is_stale_for(&nmr.data, nmr.craft_reference())
        {
            ReportStatus::NeedsReview
        } else {
            ReportStatus::Available
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnalysisReportRecord {
    pub id: ReportId,
    pub name: String,
    pub kind: ReportKindId,
    pub source: ReportSource,
    /// Tagged, domain-owned definition and resolved result snapshot.
    pub definition: serde_json::Value,
    pub snapshot: serde_json::Value,
    pub source_fingerprint: String,
    pub schema_version: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewAnalysisReport {
    pub name: String,
    pub kind: ReportKindId,
    pub source: ReportSource,
    pub definition: serde_json::Value,
    pub snapshot: serde_json::Value,
    pub source_fingerprint: String,
    pub schema_version: u32,
}

impl Document {
    pub fn allocate_report_id(&mut self) -> ReportId {
        let id = ReportId(self.next_report_id);
        self.next_report_id = self
            .next_report_id
            .checked_add(1)
            .expect("report id overflow");
        id
    }

    pub fn repair_report_allocator(&mut self) {
        let required = self
            .reports
            .iter()
            .map(|r| r.id.0.saturating_add(1))
            .max()
            .unwrap_or(0);
        self.next_report_id = self.next_report_id.max(required);
    }

    pub fn report(&self, id: ReportId) -> Option<&AnalysisReportRecord> {
        self.reports.iter().find(|r| r.id == id)
    }
    pub fn reports_for_source(
        &self,
        source: ReportSource,
    ) -> impl Iterator<Item = &AnalysisReportRecord> {
        self.reports.iter().filter(move |r| r.source == source)
    }
    pub fn create_report(&mut self, report: NewAnalysisReport) -> ReportId {
        let id = self.allocate_report_id();
        self.reports.push(AnalysisReportRecord {
            id,
            name: report.name,
            kind: report.kind,
            source: report.source,
            definition: report.definition,
            snapshot: report.snapshot,
            source_fingerprint: report.source_fingerprint,
            schema_version: report.schema_version,
        });
        self.mark_dirty();
        id
    }
    pub fn rename_report(&mut self, id: ReportId, name: String) -> Result<(), String> {
        let report = self
            .report_mut(id)
            .ok_or_else(|| "Report not found".to_owned())?;
        report.name = name;
        self.mark_dirty();
        Ok(())
    }
    pub fn copy_report(&mut self, id: ReportId, name: Option<String>) -> Result<ReportId, String> {
        let source = self
            .report(id)
            .cloned()
            .ok_or_else(|| "Report not found".to_owned())?;
        let new_id = self.allocate_report_id();
        self.reports.push(AnalysisReportRecord {
            id: new_id,
            name: name.unwrap_or_else(|| format!("{} copy", source.name)),
            ..source
        });
        self.mark_dirty();
        Ok(new_id)
    }
    pub fn update_report(&mut self, record: AnalysisReportRecord) -> Result<(), String> {
        let slot = self
            .reports
            .iter_mut()
            .find(|r| r.id == record.id)
            .ok_or_else(|| "Report not found".to_owned())?;
        *slot = record;
        self.mark_dirty();
        Ok(())
    }
    pub fn delete_report(&mut self, id: ReportId) -> bool {
        let before = self.reports.len();
        self.reports.retain(|r| r.id != id);
        let changed = before != self.reports.len();
        if changed {
            self.mark_dirty();
        }
        changed
    }
    fn report_mut(&mut self, id: ReportId) -> Option<&mut AnalysisReportRecord> {
        self.reports.iter_mut().find(|r| r.id == id)
    }
}
