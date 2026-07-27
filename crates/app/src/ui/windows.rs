//! Modal and floating application windows grouped by responsibility.

mod canvas_settings;
mod panel_note_edit;
mod project;
mod text_edit;

use super::*;

pub(super) use canvas_settings::{canvas_settings_layer, canvas_settings_window};
pub(super) use panel_note_edit::panel_note_edit_window;
pub(super) use project::{handle_close_request, quit_confirm_window, save_project_window};
pub(super) use text_edit::text_edit_window;
