use plotx_core::automation::{
    AutomationError, CallerType, ExecutionAuthority, TaskCancellation, WorkflowDefinition,
    execute_workflow, write_run_manifest,
};
use plotx_core::export::ExportFormat;
use plotx_core::state::PlotxApp;
use plotx_core::workflow::{self, InspectionReport, WorkflowError};
use serde_json::json;
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

const HELP: &str = r#"plotx-cli - headless PlotX workflows

USAGE:
  plotx-cli inspect <input> [--json]
  plotx-cli process <input> --scheme <file> --output <path> [--format svg|pdf|png|tiff|jpeg]
  plotx-cli batch --workflow <workflow.json> --manifest <manifest.json>

COMMANDS:
  inspect   Detect, load and describe one supported dataset.
  process   Load one dataset, apply one .plotxproc scheme, create a default
            canvas, and export it. If --format is omitted, infer it from output.
  batch     Execute a tool-based v1 DAG and atomically write its run manifest.

OUTPUT:
  inspect writes a stable text report, or plotx.inspect.v1 JSON with --json.
  process writes one plotx.process.v1 JSON result to stdout.
  batch writes the same plotx.run-manifest.v1 JSON saved at --manifest.
  Operational diagnostics and errors are written only to stderr.

STABLE EXIT CODES:
  0   Success or help.
  2   Invalid command-line usage or workflow definition.
  3   Input detection/loading or workflow file reading failed.
  4   Scheme loading, compatibility or processing failed.
  5   Default canvas or figure construction failed.
  6   Export or output writing failed.
  7   Batch completed, but one or more inputs failed.
  70  Internal serialization failure.
"#;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Success = 0,
    Usage = 2,
    Input = 3,
    Scheme = 4,
    Canvas = 5,
    Export = 6,
    BatchFailed = 7,
    Internal = 70,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Command {
    Inspect {
        input: PathBuf,
        json: bool,
    },
    Process {
        input: PathBuf,
        scheme: PathBuf,
        output: PathBuf,
        format: OutputFormat,
    },
    Batch {
        workflow: PathBuf,
        manifest: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OutputFormat(ExportFormat);

impl OutputFormat {
    fn explicit(value: &OsStr) -> Result<Self, ParseError> {
        match value.to_str() {
            Some("svg") => Ok(Self(ExportFormat::Svg)),
            Some("pdf") => Ok(Self(ExportFormat::Pdf)),
            Some("png") => Ok(Self(ExportFormat::Png)),
            Some("tiff") => Ok(Self(ExportFormat::Tiff)),
            Some("jpeg") => Ok(Self(ExportFormat::Jpeg)),
            _ => Err(ParseError::new(
                "--format must be one of: svg, pdf, png, tiff, jpeg",
            )),
        }
    }

    fn infer(path: &Path) -> Result<Self, ParseError> {
        let extension = path
            .extension()
            .and_then(OsStr::to_str)
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("svg") => Ok(Self(ExportFormat::Svg)),
            Some("pdf") => Ok(Self(ExportFormat::Pdf)),
            Some("png") => Ok(Self(ExportFormat::Png)),
            Some("tif" | "tiff") => Ok(Self(ExportFormat::Tiff)),
            Some("jpg" | "jpeg") => Ok(Self(ExportFormat::Jpeg)),
            _ => Err(ParseError::new(
                "cannot infer output format; add --format svg|pdf|png|tiff|jpeg",
            )),
        }
    }

    fn name(self) -> &'static str {
        match self.0 {
            ExportFormat::Svg => "svg",
            ExportFormat::Pdf => "pdf",
            ExportFormat::Png => "png",
            ExportFormat::Tiff => "tiff",
            ExportFormat::Jpeg => "jpeg",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flag {
    Help,
    Json,
    Scheme,
    Output,
    Format,
    Workflow,
    Manifest,
}

impl Flag {
    fn parse(value: &OsStr) -> Result<Option<Self>, ParseError> {
        match value.to_str() {
            Some("--help" | "-h") => Ok(Some(Self::Help)),
            Some("--json") => Ok(Some(Self::Json)),
            Some("--scheme") => Ok(Some(Self::Scheme)),
            Some("--output") => Ok(Some(Self::Output)),
            Some("--format") => Ok(Some(Self::Format)),
            Some("--workflow") => Ok(Some(Self::Workflow)),
            Some("--manifest") => Ok(Some(Self::Manifest)),
            Some(value) if value.starts_with('-') => {
                Err(ParseError::new(format!("unknown option: {value}")))
            }
            _ => Ok(None),
        }
    }

    fn spelling(self) -> &'static str {
        match self {
            Self::Help => "--help",
            Self::Json => "--json",
            Self::Scheme => "--scheme",
            Self::Output => "--output",
            Self::Format => "--format",
            Self::Workflow => "--workflow",
            Self::Manifest => "--manifest",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ParseOutcome {
    Command(Command),
    Help,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ParseError(String);

impl ParseError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

fn parse_args<I>(args: I) -> Result<ParseOutcome, ParseError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args: VecDeque<OsString> = args.into_iter().collect();
    let _program = args.pop_front();
    let Some(command) = args.pop_front() else {
        return Err(ParseError::new("a command is required"));
    };
    match command.to_str() {
        Some("--help" | "-h") => Ok(ParseOutcome::Help),
        Some("inspect") => parse_inspect(args),
        Some("process") => parse_process(args),
        Some("batch") => parse_batch(args),
        Some(value) => Err(ParseError::new(format!("unknown command: {value}"))),
        None => Err(ParseError::new("command is not valid Unicode")),
    }
}

fn parse_batch(mut args: VecDeque<OsString>) -> Result<ParseOutcome, ParseError> {
    let mut workflow = None;
    let mut manifest = None;
    while let Some(token) = args.pop_front() {
        match Flag::parse(&token)? {
            Some(Flag::Help) => return Ok(ParseOutcome::Help),
            Some(Flag::Workflow) if workflow.is_none() => {
                workflow = Some(PathBuf::from(take_value(&mut args, Flag::Workflow)?));
            }
            Some(Flag::Manifest) if manifest.is_none() => {
                manifest = Some(PathBuf::from(take_value(&mut args, Flag::Manifest)?));
            }
            Some(flag) => {
                return Err(ParseError::new(format!(
                    "{} is invalid or was provided more than once for batch",
                    flag.spelling()
                )));
            }
            None => {
                return Err(ParseError::new(
                    "batch accepts inputs only through --workflow",
                ));
            }
        }
    }
    Ok(ParseOutcome::Command(Command::Batch {
        workflow: workflow
            .ok_or_else(|| ParseError::new("batch requires --workflow <workflow.json>"))?,
        manifest: manifest
            .ok_or_else(|| ParseError::new("batch requires --manifest <manifest.json>"))?,
    }))
}

fn parse_inspect(mut args: VecDeque<OsString>) -> Result<ParseOutcome, ParseError> {
    let mut input = None;
    let mut json = false;
    while let Some(token) = args.pop_front() {
        match Flag::parse(&token)? {
            Some(Flag::Help) => return Ok(ParseOutcome::Help),
            Some(Flag::Json) if !json => json = true,
            Some(Flag::Json) => return Err(ParseError::new("--json was provided more than once")),
            Some(flag) => {
                return Err(ParseError::new(format!(
                    "{} is not valid for inspect",
                    flag.spelling()
                )));
            }
            None if input.is_none() => input = Some(PathBuf::from(token)),
            None => return Err(ParseError::new("inspect accepts exactly one input")),
        }
    }
    Ok(ParseOutcome::Command(Command::Inspect {
        input: input.ok_or_else(|| ParseError::new("inspect requires <input>"))?,
        json,
    }))
}

fn parse_process(mut args: VecDeque<OsString>) -> Result<ParseOutcome, ParseError> {
    let mut input = None;
    let mut scheme = None;
    let mut output = None;
    let mut format = None;
    while let Some(token) = args.pop_front() {
        match Flag::parse(&token)? {
            Some(Flag::Help) => return Ok(ParseOutcome::Help),
            Some(Flag::Scheme) if scheme.is_none() => {
                scheme = Some(PathBuf::from(take_value(&mut args, Flag::Scheme)?));
            }
            Some(Flag::Output) if output.is_none() => {
                output = Some(PathBuf::from(take_value(&mut args, Flag::Output)?));
            }
            Some(Flag::Format) if format.is_none() => {
                format = Some(OutputFormat::explicit(&take_value(
                    &mut args,
                    Flag::Format,
                )?)?);
            }
            Some(Flag::Json) => return Err(ParseError::new("--json is not valid for process")),
            Some(flag) => {
                return Err(ParseError::new(format!(
                    "{} was provided more than once",
                    flag.spelling()
                )));
            }
            None if input.is_none() => input = Some(PathBuf::from(token)),
            None => return Err(ParseError::new("process accepts exactly one input")),
        }
    }
    let input = input.ok_or_else(|| ParseError::new("process requires <input>"))?;
    let scheme = scheme.ok_or_else(|| ParseError::new("process requires --scheme <file>"))?;
    let output = output.ok_or_else(|| ParseError::new("process requires --output <path>"))?;
    let format = format
        .map(Ok)
        .unwrap_or_else(|| OutputFormat::infer(&output))?;
    Ok(ParseOutcome::Command(Command::Process {
        input,
        scheme,
        output,
        format,
    }))
}

fn take_value(args: &mut VecDeque<OsString>, flag: Flag) -> Result<OsString, ParseError> {
    let value = args
        .pop_front()
        .ok_or_else(|| ParseError::new(format!("{} requires a value", flag.spelling())))?;
    if Flag::parse(&value)?.is_some() {
        return Err(ParseError::new(format!(
            "{} requires a value before the next option",
            flag.spelling()
        )));
    }
    Ok(value)
}

fn main() -> std::process::ExitCode {
    let status = match parse_args(std::env::args_os()) {
        Ok(ParseOutcome::Help) => {
            print!("{HELP}");
            Status::Success
        }
        Ok(ParseOutcome::Command(command)) => run(command),
        Err(error) => {
            eprintln!("plotx-cli: {}\n\n{HELP}", error.0);
            Status::Usage
        }
    };
    std::process::ExitCode::from(status as u8)
}

fn run(command: Command) -> Status {
    match command {
        Command::Inspect { input, json } => {
            eprintln!("plotx-cli: loading {}", input.display());
            match workflow::load_dataset(&input) {
                Ok(loaded) => {
                    emit_warnings(&loaded.inspection);
                    let result = if json {
                        serde_json::to_string_pretty(&loaded.inspection)
                    } else {
                        Ok(text_report(&loaded.inspection))
                    };
                    match result {
                        Ok(output) => {
                            println!("{output}");
                            Status::Success
                        }
                        Err(error) => {
                            eprintln!("plotx-cli: result serialization failed: {error}");
                            Status::Internal
                        }
                    }
                }
                Err(error) => fail(error),
            }
        }
        Command::Process {
            input,
            scheme,
            output,
            format,
        } => {
            eprintln!(
                "plotx-cli: processing {} with {}",
                input.display(),
                scheme.display()
            );
            match workflow::process_file(&input, &scheme, &output, format.0) {
                Ok(result) => {
                    emit_warnings(&result.inspection);
                    let value = json!({
                        "schema": "plotx.process.v1",
                        "format": format.name(),
                        "output_paths": result.output_paths,
                        "warnings": result.inspection.warnings,
                    });
                    match serde_json::to_string(&value) {
                        Ok(output) => {
                            println!("{output}");
                            Status::Success
                        }
                        Err(error) => {
                            eprintln!("plotx-cli: result serialization failed: {error}");
                            Status::Internal
                        }
                    }
                }
                Err(error) => fail(error),
            }
        }
        Command::Batch { workflow, manifest } => {
            eprintln!("plotx-cli: running workflow {}", workflow.display());
            match run_automation_workflow(&workflow) {
                Ok(result) => {
                    if let Err(error) = write_run_manifest(&manifest, &result) {
                        eprintln!("plotx-cli: manifest write failed: {error}");
                        return Status::Export;
                    }
                    match serde_json::to_string(&result) {
                        Ok(output) => {
                            println!("{output}");
                            if result.errors.is_empty() && !result.cancelled {
                                Status::Success
                            } else {
                                Status::BatchFailed
                            }
                        }
                        Err(error) => {
                            eprintln!("plotx-cli: result serialization failed: {error}");
                            Status::Internal
                        }
                    }
                }
                Err(error) => fail_automation(error),
            }
        }
    }
}

fn run_automation_workflow(
    path: &Path,
) -> Result<plotx_core::automation::RunManifest, AutomationError> {
    let bytes = std::fs::read(path).map_err(|source| AutomationError::Io {
        path: path.to_owned(),
        source,
    })?;
    let mut workflow: WorkflowDefinition = serde_json::from_slice(&bytes)
        .map_err(|error| AutomationError::InvalidWorkflow(error.to_string()))?;
    workflow.resolve_paths_from(path);
    let mut app = PlotxApp::new();
    execute_workflow(
        &mut app,
        &workflow,
        CallerType::Workflow,
        ExecutionAuthority::ExternalWrite,
        &TaskCancellation::default(),
        &mut |_| {},
    )
}

fn fail_automation(error: AutomationError) -> Status {
    eprintln!("plotx-cli: {error}");
    match error {
        AutomationError::Io { .. } => Status::Input,
        AutomationError::InvalidWorkflow(_)
        | AutomationError::UnknownTool(_)
        | AutomationError::ToolVersion { .. }
        | AutomationError::InvalidParameters { .. }
        | AutomationError::InvalidSelector(_) => Status::Usage,
        AutomationError::Execution(_)
        | AutomationError::StaleRevision { .. }
        | AutomationError::InsufficientAuthority { .. } => Status::BatchFailed,
    }
}

fn fail(error: WorkflowError) -> Status {
    let status = match &error {
        WorkflowError::Load(_) => Status::Input,
        WorkflowError::Scheme(_) | WorkflowError::Processing(_) | WorkflowError::Integration(_) => {
            Status::Scheme
        }
        WorkflowError::FigureUnavailable(_) => Status::Canvas,
        WorkflowError::Export(_) => Status::Export,
    };
    eprintln!("plotx-cli: {error}");
    status
}

fn emit_warnings(report: &InspectionReport) {
    for warning in &report.warnings {
        eprintln!("plotx-cli: warning [{}]: {}", warning.code, warning.message);
    }
}

fn text_report(report: &InspectionReport) -> String {
    let mut lines = vec![
        format!("schema: {}", report.schema),
        format!("format: {}", report.format),
        format!("dimension.count: {}", report.dimension.count),
        format!(
            "dimension.shape: {}",
            report
                .dimension
                .shape
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join("x")
        ),
        format!("domain: {}", report.domain),
        format!(
            "provenance.selected_path: {}",
            report.provenance.selected_path.display()
        ),
        format!(
            "provenance.data_path: {}",
            report.provenance.data_path.display()
        ),
    ];
    for path in &report.provenance.parameter_paths {
        lines.push(format!("provenance.parameter_path: {}", path.display()));
    }
    if let Some(ephys) = &report.electrophysiology {
        lines.push(format!("abf.version: {}", ephys.abf_version));
        lines.push(format!("sample_rate_hz: {}", ephys.sample_rate_hz));
        lines.push(format!("sweeps: {}", ephys.sweep_count));
        lines.push(format!("channels: {}", ephys.channels.join(", ")));
        lines.push(format!("units: {}", ephys.units.join(", ")));
        lines.push(format!(
            "protocol: {}",
            ephys.protocol.as_deref().unwrap_or("unknown")
        ));
    }
    for warning in &report.warnings {
        lines.push(format!("warning.{}: {}", warning.code, warning.message));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> Result<ParseOutcome, ParseError> {
        parse_args(values.iter().map(OsString::from))
    }

    #[test]
    fn inspect_parser_accepts_json_on_either_side_of_input() {
        let expected = ParseOutcome::Command(Command::Inspect {
            input: "sample.jdf".into(),
            json: true,
        });
        assert_eq!(
            parse(&["plotx-cli", "inspect", "--json", "sample.jdf"]),
            Ok(expected.clone())
        );
        assert_eq!(
            parse(&["plotx-cli", "inspect", "sample.jdf", "--json"]),
            Ok(expected)
        );
    }

    #[test]
    fn process_parser_infers_format_and_requires_named_paths() {
        assert_eq!(
            parse(&[
                "plotx-cli",
                "process",
                "sample.jdf",
                "--scheme",
                "routine.plotxproc",
                "--output",
                "figure.svg",
            ]),
            Ok(ParseOutcome::Command(Command::Process {
                input: "sample.jdf".into(),
                scheme: "routine.plotxproc".into(),
                output: "figure.svg".into(),
                format: OutputFormat(ExportFormat::Svg),
            }))
        );
        assert!(parse(&["plotx-cli", "process", "sample.jdf"]).is_err());
    }

    #[test]
    fn batch_parser_requires_workflow_and_manifest_paths() {
        assert_eq!(
            parse(&[
                "plotx-cli",
                "batch",
                "--workflow",
                "workflow.json",
                "--manifest",
                "run.json",
            ]),
            Ok(ParseOutcome::Command(Command::Batch {
                workflow: "workflow.json".into(),
                manifest: "run.json".into(),
            }))
        );
        assert!(parse(&["plotx-cli", "batch", "workflow.json"]).is_err());
    }

    #[test]
    fn workflow_errors_map_to_stable_exit_categories() {
        let status = fail(WorkflowError::FigureUnavailable("NMR 1D"));
        assert_eq!(status, Status::Canvas);
        assert_eq!(Status::Usage as u8, 2);
        assert_eq!(Status::Export as u8, 6);
    }
}
