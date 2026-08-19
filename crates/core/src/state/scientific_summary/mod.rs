mod resolver;

pub use resolver::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryPart {
    pub semantic_key: String,
    pub text: String,
}

impl SummaryPart {
    pub fn new(semantic_key: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            semantic_key: semantic_key.into(),
            text: text.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScientificSummary {
    pub subject: SummaryPart,
    pub observation: SummaryPart,
    pub context: Option<SummaryPart>,
}

impl ScientificSummary {
    pub fn parts(&self) -> Vec<SummaryPart> {
        std::iter::once(self.subject.clone())
            .chain(std::iter::once(self.observation.clone()))
            .chain(self.context.clone())
            .collect()
    }

    pub fn format(&self) -> String {
        format_parts(&self.parts())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryLine {
    pub panel_label: Option<String>,
    pub parts: Vec<SummaryPart>,
}

impl SummaryLine {
    pub fn format(&self) -> String {
        let body = format_parts(&self.parts);
        match (&self.panel_label, body.is_empty()) {
            (Some(label), false) => format!("{label} — {body}"),
            (_, false) => body,
            _ => String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CanvasScientificSummary {
    pub lines: Vec<SummaryLine>,
}

impl CanvasScientificSummary {
    pub fn formatted_lines(&self) -> Vec<String> {
        self.lines
            .iter()
            .map(SummaryLine::format)
            .filter(|line| !line.is_empty())
            .collect()
    }
}

fn format_parts(parts: &[SummaryPart]) -> String {
    parts
        .iter()
        .map(|part| part.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Dataset, NmrDataset, PlotxApp};
    use num_complex::Complex64;
    use plotx_io::{Domain, ImportedScientificIdentity, NmrData};

    fn nmr_dataset() -> Dataset {
        let mut dataset = NmrDataset::load(NmrData {
            points: vec![Complex64::new(1.0, 0.0); 8],
            domain: Domain::Frequency,
            spectral_width_hz: 4_000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 4.7,
            nucleus: "1H".to_owned(),
            source: "raw/exp1/fid".to_owned(),
            group_delay: 0.0,
        });
        dataset.scientific_identity = ImportedScientificIdentity {
            subject: Some("Sample A".to_owned()),
            acquisition: Some("zg30".to_owned()),
            source_label: "fid".to_owned(),
        };
        Dataset::Nmr(Box::new(dataset))
    }

    #[test]
    fn nmr_summary_contains_only_the_v1_scientific_contract() {
        let dataset = nmr_dataset();
        let mut app = PlotxApp::default();
        app.doc
            .canvases
            .push(crate::workflow::build_default_canvas(&dataset, "fid"));
        app.doc.datasets.push(dataset);

        assert_eq!(
            app.canvas_scientific_summary(0).formatted_lines(),
            vec!["Sample A · 1H · zg30"]
        );
    }

    #[test]
    fn equal_text_with_distinct_semantics_is_not_silently_dropped() {
        let summary = ScientificSummary {
            subject: SummaryPart::new("subject:a", "A"),
            observation: SummaryPart::new("observation:a", "A"),
            context: None,
        };
        assert_eq!(summary.format(), "A · A");
    }
}
