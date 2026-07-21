use plotx_core::operation::Diagnostic;
use std::path::PathBuf;

pub(crate) enum DelimitedTableSource {
    File(PathBuf),
    Clipboard,
}

impl DelimitedTableSource {
    pub(super) fn recent_path(&self) -> Option<PathBuf> {
        match self {
            Self::File(path) => Some(path.clone()),
            Self::Clipboard => None,
        }
    }

    pub(super) fn dataset_name(&self) -> String {
        match self {
            Self::File(path) => path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| !stem.is_empty())
                .unwrap_or("Imported table")
                .to_owned(),
            Self::Clipboard => "Clipboard table".to_owned(),
        }
    }

    pub(super) fn add_diagnostic_context(&self, diagnostic: Diagnostic) -> Diagnostic {
        match self {
            Self::File(path) => diagnostic
                .with_context("input_source", "file")
                .with_context("path", path.display().to_string()),
            Self::Clipboard => diagnostic.with_context("input_source", "clipboard"),
        }
    }

    pub(super) fn retained_source(
        &self,
        input: &str,
        delimiter: plotx_core::delimited::Delimiter,
    ) -> plotx_core::state::TableImportSource {
        let media_type = match delimiter {
            plotx_core::delimited::Delimiter::Comma => "text/csv",
            plotx_core::delimited::Delimiter::Tab => "text/tab-separated-values",
            plotx_core::delimited::Delimiter::Semicolon => "text/csv; delimiter=semicolon",
        };
        let mut retained = plotx_core::state::TableImportSource::new(
            std::sync::Arc::<[u8]>::from(input.as_bytes()),
            media_type,
        );
        retained.metadata.insert(
            "space.nmrtist.plotx.import.delimiter".into(),
            serde_json::Value::String(delimiter.to_string()),
        );
        match self {
            Self::File(path) => {
                retained.name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned);
                retained.metadata.insert(
                    "space.nmrtist.plotx.import.source".into(),
                    serde_json::Value::String("file".into()),
                );
            }
            Self::Clipboard => {
                retained.name = Some("clipboard.tsv".into());
                retained.metadata.insert(
                    "space.nmrtist.plotx.import.source".into(),
                    serde_json::Value::String("clipboard".into()),
                );
            }
        }
        retained
    }
}
