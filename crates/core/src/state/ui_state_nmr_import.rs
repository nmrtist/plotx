use plotx_io::nmr_sampling::{IndexBase, SamplingDeclaration};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct NmrImportDraft {
    pub path: PathBuf,
    pub assertion_id: String,
    pub source: String,
    pub grid: String,
    pub lanes: String,
    pub one_based: Option<bool>,
    pub rows: String,
    pub error: Option<String>,
}

impl NmrImportDraft {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            assertion_id: format!("plotx-user-schedule-{}", uuid::Uuid::new_v4()),
            source: String::new(),
            grid: String::new(),
            lanes: String::new(),
            one_based: None,
            rows: String::new(),
            error: None,
        }
    }

    pub fn declaration(&self) -> Result<SamplingDeclaration, String> {
        let number = |text: &str, name: &str| {
            text.trim()
                .parse::<usize>()
                .map_err(|_| format!("Enter an integer for {name}."))
        };
        let index_base = match self.one_based {
            Some(true) => IndexBase::One,
            Some(false) => IndexBase::Zero,
            None => return Err("Select the sampling table's index base.".into()),
        };
        let coordinates = self
            .rows
            .lines()
            .filter(|row| !row.trim().is_empty())
            .map(|row| number(row, "each observation (one per line)").map(|value| vec![value]))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SamplingDeclaration {
            assertion_id: self.assertion_id.clone(),
            source: self.source.clone(),
            grid_shape: vec![number(&self.grid, "the original indirect grid")?],
            component_counts: vec![number(&self.lanes, "lanes per observation")?],
            coordinates,
            index_base,
        })
    }
}
