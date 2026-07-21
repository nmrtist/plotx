//! Clipboard-driven delimited table import.
//!
//! Clipboard access remains owned by the egui backend. A user command first
//! sends `RequestPaste`; a later frame consumes the resulting `Event::Paste`.

use std::time::Duration;

use egui::{Context, Event, ViewportCommand};
use plotx_core::operation::{Diagnostic, DiagnosticCode, OperationKind, OperationReport, Severity};
use plotx_core::state::PlotxApp;

use super::file_dialogs::{DelimitedTableSource, import_delimited_text_with_schema};

const PASTE_TIMEOUT_SECONDS: f64 = 1.5;

#[derive(Debug, Default, PartialEq)]
enum PastePhase {
    #[default]
    Idle,
    Awaiting {
        requested_at: f64,
    },
}

#[derive(Debug, PartialEq)]
enum PastePoll {
    Inactive,
    Pending,
    Ready(String),
    TimedOut,
}

#[derive(Debug, Default)]
pub(crate) struct ClipboardTablePaste {
    phase: PastePhase,
    schema_json: Option<String>,
}

impl ClipboardTablePaste {
    pub(crate) fn request(&mut self, app: &mut PlotxApp, ctx: &Context) {
        let requested_at = ctx.input(|input| input.time);
        self.phase = PastePhase::Awaiting { requested_at };
        #[cfg(windows)]
        {
            self.schema_json = super::clipboard_native::get_table_schema().ok().flatten();
        }
        #[cfg(not(windows))]
        {
            self.schema_json = None;
        }
        app.session.status = "Waiting for delimited text from the clipboard…".to_owned();
        ctx.send_viewport_cmd(ViewportCommand::RequestPaste);
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    pub(crate) fn begin_frame(&mut self, app: &mut PlotxApp, ctx: &Context) {
        if self.phase == PastePhase::Idle && take_table_paste_shortcut(ctx) {
            self.request(app, ctx);
            return;
        }

        let pasted = if matches!(self.phase, PastePhase::Awaiting { .. }) {
            take_paste_event(ctx)
        } else {
            None
        };
        let now = ctx.input(|input| input.time);
        match poll_phase(&mut self.phase, pasted, now) {
            PastePoll::Ready(text) => {
                import_delimited_text_with_schema(
                    app,
                    &text,
                    DelimitedTableSource::Clipboard,
                    self.schema_json.take().as_deref(),
                );
            }
            PastePoll::Pending => {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            PastePoll::TimedOut => {
                self.schema_json = None;
                record_clipboard_refusal(app);
            }
            PastePoll::Inactive => {}
        }
    }
}

fn take_table_paste_shortcut(ctx: &Context) -> bool {
    if ctx.egui_wants_keyboard_input() {
        return false;
    }
    ctx.input_mut(|input| {
        if !(input.modifiers.command && input.modifiers.shift) {
            return false;
        }
        let Some(index) = input
            .events
            .iter()
            .position(|event| matches!(event, Event::Paste(_)))
        else {
            return false;
        };
        input.events.remove(index);
        true
    })
}

fn take_paste_event(ctx: &Context) -> Option<String> {
    ctx.input_mut(|input| {
        let index = input
            .events
            .iter()
            .position(|event| matches!(event, Event::Paste(_)))?;
        match input.events.remove(index) {
            Event::Paste(text) => Some(text),
            _ => unreachable!("paste event index must still contain a paste event"),
        }
    })
}

fn poll_phase(phase: &mut PastePhase, pasted: Option<String>, now: f64) -> PastePoll {
    let PastePhase::Awaiting { requested_at } = phase else {
        return PastePoll::Inactive;
    };
    if let Some(text) = pasted {
        *phase = PastePhase::Idle;
        return PastePoll::Ready(text);
    }
    if now - *requested_at >= PASTE_TIMEOUT_SECONDS {
        *phase = PastePhase::Idle;
        return PastePoll::TimedOut;
    }
    PastePoll::Pending
}

fn record_clipboard_refusal(app: &mut PlotxApp) {
    let operation_id = app.session.begin_operation();
    app.session.record_operation(OperationReport::<()>::failure(
        operation_id,
        OperationKind::TableImport,
        "No text was returned by the platform clipboard.",
        Diagnostic::new(
            Severity::Error,
            DiagnosticCode::TableImportFailed,
            "The clipboard paste request returned no text. Copy the table and try Paste table again.",
        )
        .with_source("app.table_import")
        .with_context("input_source", "clipboard")
        .with_context("stage", "clipboard_request")
        .with_context("retryable", "true"),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awaiting_phase_accepts_paste_and_returns_to_idle() {
        let mut phase = PastePhase::Awaiting { requested_at: 10.0 };
        let result = poll_phase(&mut phase, Some("x\ty\n0\t1".to_owned()), 10.1);
        assert_eq!(result, PastePoll::Ready("x\ty\n0\t1".to_owned()));
        assert_eq!(phase, PastePhase::Idle);
    }

    #[test]
    fn awaiting_phase_remains_recoverable_until_timeout() {
        let mut phase = PastePhase::Awaiting { requested_at: 10.0 };
        assert_eq!(poll_phase(&mut phase, None, 10.5), PastePoll::Pending);
        assert_eq!(
            poll_phase(&mut phase, None, 10.0 + PASTE_TIMEOUT_SECONDS),
            PastePoll::TimedOut
        );
        assert_eq!(phase, PastePhase::Idle);
    }

    #[test]
    fn idle_phase_never_claims_unsolicited_clipboard_text() {
        let mut phase = PastePhase::Idle;
        assert_eq!(
            poll_phase(&mut phase, Some("leave me for TextEdit".to_owned()), 1.0),
            PastePoll::Inactive
        );
        assert_eq!(phase, PastePhase::Idle);
    }
}
