//! macOS system menu bridge. The native items contain no business logic: they
//! emit stable IDs that are resolved back to the shared command dispatcher.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};

use muda::accelerator::{Accelerator, CMD_OR_CTRL, Code, Modifiers};
use muda::{AboutMetadata, CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use plotx_core::state::PlotxApp;

use super::clipboard_table::ClipboardTablePaste;
use super::commands::{self, CommandId};
use super::menus::{self, MenuEntry};

enum NativeItem {
    Plain(MenuItem),
    Check(CheckMenuItem),
}

impl NativeItem {
    fn sync(&self, enabled: bool, checked: bool) {
        match self {
            Self::Plain(item) => {
                if item.is_enabled() != enabled {
                    item.set_enabled(enabled);
                }
            }
            Self::Check(item) => {
                if item.is_enabled() != enabled {
                    item.set_enabled(enabled);
                }
                if item.is_checked() != checked {
                    item.set_checked(checked);
                }
            }
        }
    }
}

pub(crate) struct NativeMenu {
    _menu: Menu,
    receiver: Receiver<String>,
    events: HashMap<String, CommandId>,
    tracked: Vec<(CommandId, NativeItem)>,
    /// The submenu hosting the dynamic recent-files entries. AppKit items are
    /// built once, so `sync_recent` rebuilds these rows whenever the list
    /// changes instead of routing them through `tracked` (labels change too,
    /// not just enabled state).
    recent_menu: Option<Submenu>,
    recent_items: Vec<MenuItem>,
    /// `None` until the first sync, so an initially empty list still gets its
    /// placeholder row built.
    recent_cache: Option<Vec<std::path::PathBuf>>,
}

impl NativeMenu {
    pub(crate) fn new(app: &PlotxApp, ctx: &egui::Context) -> Result<Self, muda::Error> {
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
            // A disconnected receiver means the application is already tearing
            // down, so no diagnostic or repaint is useful at that point.
            if sender.send(event.id.0).is_ok() {
                repaint.request_repaint();
            }
        }));

        let menu = Menu::new();
        let mut native = Self {
            _menu: menu,
            receiver,
            events: HashMap::new(),
            tracked: Vec::new(),
            recent_menu: None,
            recent_items: Vec::new(),
            recent_cache: None,
        };
        let application = Submenu::new("PlotX", true);
        native.build_application(app, &application)?;
        native._menu.append(&application)?;

        // The shared spec drives this bar and the in-window egui bar alike;
        // only the Window menu and the macOS ordering (Window before Help)
        // are added here.
        let (help_menus, main_menus): (Vec<_>, Vec<_>) = menus::menu_bar_spec()
            .into_iter()
            .partition(|(title, _)| *title == "Help");
        for (title, entries) in &main_menus {
            let submenu = Submenu::new(title, true);
            native.append_entries(app, &submenu, entries)?;
            native._menu.append(&submenu)?;
        }
        let window = Submenu::new("Window", true);
        build_window(&window)?;
        native._menu.append(&window)?;
        let help = Submenu::new("Help", true);
        for (_, entries) in &help_menus {
            native.append_entries(app, &help, entries)?;
        }
        native._menu.append(&help)?;

        native._menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        help.set_as_help_menu_for_nsapp();
        native.sync_recent(app);
        Ok(native)
    }

    pub(crate) fn poll(
        &mut self,
        app: &mut PlotxApp,
        clipboard: &mut ClipboardTablePaste,
        ctx: &egui::Context,
    ) {
        // AppKit fires menu key equivalents before egui ever sees the
        // keystroke, so mirror the focus gate every egui shortcut handler
        // applies: while a text field owns the keyboard, chords like Cmd+A or
        // Cmd+Z must not act on the canvas. The palette toggle stays exempt,
        // matching `handle_palette_shortcut`.
        let typing = ctx.egui_wants_keyboard_input();
        self.sync_recent(app);
        while let Ok(stable_id) = self.receiver.try_recv() {
            let Some(&id) = self.events.get(&stable_id) else {
                continue;
            };
            if typing && id != CommandId::CommandPalette {
                continue;
            }
            commands::execute(id, app, clipboard, ctx);
        }
        for (id, item) in &self.tracked {
            let state = commands::describe(app, *id);
            item.sync(state.enabled, state.checked == Some(true));
        }
    }

    fn build_application(&mut self, app: &PlotxApp, menu: &Submenu) -> Result<(), muda::Error> {
        let about = PredefinedMenuItem::about(
            Some("About PlotX"),
            Some(AboutMetadata {
                name: Some("PlotX".to_owned()),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
                copyright: Some("Copyright © 2026 Jiekang Tian".to_owned()),
                ..Default::default()
            }),
        );
        menu.append(&about)?;
        separator(menu)?;
        self.append(app, menu, CommandId::Preferences)?;
        separator(menu)?;
        menu.append(&PredefinedMenuItem::services(None))?;
        separator(menu)?;
        menu.append(&PredefinedMenuItem::hide(Some("Hide PlotX")))?;
        menu.append(&PredefinedMenuItem::hide_others(None))?;
        menu.append(&PredefinedMenuItem::show_all(None))?;
        separator(menu)?;
        // A custom item intentionally replaces PredefinedMenuItem::quit: its
        // Close command passes through PlotX's Save / Discard / Cancel guard.
        self.append(app, menu, CommandId::Quit)
    }

    /// Rebuild the recent-files rows when the list has changed since the last
    /// frame. Menu mutation failures are ignored deliberately: the commands
    /// stay reachable through the palette and the in-app surfaces, and a
    /// cosmetic AppKit hiccup must not take the poll loop down.
    fn sync_recent(&mut self, app: &PlotxApp) {
        if self
            .recent_cache
            .as_ref()
            .is_some_and(|cache| *cache == app.session.recent_files)
        {
            return;
        }
        let Some(menu) = self.recent_menu.clone() else {
            return;
        };
        for item in self.recent_items.drain(..) {
            let _ = menu.remove(&item);
        }
        // Prune the previous rows' dispatch keys so a shrinking list cannot
        // leave dead entries in the event map.
        if let Some(cache) = &self.recent_cache {
            for index in 0..cache.len() {
                self.events
                    .remove(&CommandId::OpenRecent(index).stable_id());
            }
        }
        if app.session.recent_files.is_empty() {
            // Platform convention: a disabled placeholder row, matching the
            // egui bar's "No recent files yet." (never registered in `events`).
            let item = MenuItem::with_id("file.open_recent.empty", "No Recent Files", false, None);
            if menu.insert(&item, 0).is_ok() {
                self.recent_items.push(item);
            }
        }
        for index in 0..app.session.recent_files.len() {
            let id = CommandId::OpenRecent(index);
            let state = commands::describe(app, id);
            let stable_id = id.stable_id();
            // The submenu is titled "Open Recent", so the row shows the bare
            // entry name; the palette keeps the prefixed catalog label.
            let label = commands::recent_entry_label(app, index).unwrap_or(state.label);
            let item = MenuItem::with_id(stable_id.clone(), &label, state.enabled, None);
            if menu.insert(&item, index).is_ok() {
                self.events.insert(stable_id, id);
                self.recent_items.push(item);
            }
        }
        self.recent_cache = Some(app.session.recent_files.clone());
    }

    /// Renders one level of the shared spec. Commands macOS hosts in the
    /// application menu are skipped, and separators collapse around them so no
    /// menu starts or ends with a divider.
    fn append_entries(
        &mut self,
        app: &PlotxApp,
        menu: &Submenu,
        entries: &[MenuEntry],
    ) -> Result<(), muda::Error> {
        let mut pending_separator = false;
        let mut any = false;
        for entry in entries {
            match entry {
                MenuEntry::Separator => pending_separator = true,
                MenuEntry::Command(id) if hosted_in_app_menu(*id) => {}
                MenuEntry::Command(id) => {
                    if std::mem::take(&mut pending_separator) && any {
                        separator(menu)?;
                    }
                    self.append(app, menu, *id)?;
                    any = true;
                }
                MenuEntry::Submenu(title, children) => {
                    if std::mem::take(&mut pending_separator) && any {
                        separator(menu)?;
                    }
                    let submenu = Submenu::new(*title, true);
                    self.append_entries(app, &submenu, children)?;
                    menu.append(&submenu)?;
                    any = true;
                }
                MenuEntry::RecentFiles => {
                    if std::mem::take(&mut pending_separator) && any {
                        separator(menu)?;
                    }
                    // `sync_recent` inserts the live rows at the top of this
                    // submenu; counting the placeholder keeps the separator
                    // before Clear even while the list is empty.
                    self.recent_menu = Some(menu.clone());
                    any = true;
                }
            }
        }
        Ok(())
    }

    fn append(&mut self, app: &PlotxApp, menu: &Submenu, id: CommandId) -> Result<(), muda::Error> {
        let state = commands::describe(app, id);
        let stable_id = id.stable_id();
        // A descriptor with toggle state renders as a check item; the same
        // rule the palette, menus and Ribbon apply.
        let item = if state.checked.is_some() {
            let item = CheckMenuItem::with_id(
                stable_id.clone(),
                &state.label,
                state.enabled,
                state.checked == Some(true),
                accelerator(id),
            );
            menu.append(&item)?;
            NativeItem::Check(item)
        } else {
            let item = MenuItem::with_id(
                stable_id.clone(),
                &state.label,
                state.enabled,
                accelerator(id),
            );
            menu.append(&item)?;
            NativeItem::Plain(item)
        };
        self.events.insert(stable_id, id);
        self.tracked.push((id, item));
        Ok(())
    }
}

/// Commands whose macOS home is the application menu (or a predefined item
/// there), not their position in the shared menu tree.
fn hosted_in_app_menu(id: CommandId) -> bool {
    matches!(
        id,
        CommandId::Quit | CommandId::Preferences | CommandId::About
    )
}

fn build_window(menu: &Submenu) -> Result<(), muda::Error> {
    menu.append(&PredefinedMenuItem::minimize(None))?;
    menu.append(&PredefinedMenuItem::fullscreen(None))?;
    separator(menu)?;
    menu.append(&PredefinedMenuItem::bring_all_to_front(None))
}

fn separator(menu: &Submenu) -> Result<(), muda::Error> {
    menu.append(&PredefinedMenuItem::separator())
}

/// Menu key equivalents derive from the shared binding table in `shortcuts`,
/// so the shown accelerator always matches the chord the app dispatches.
fn accelerator(id: CommandId) -> Option<Accelerator> {
    let binding = super::shortcuts::binding(id)?;
    if !binding.menu_accelerator {
        return None;
    }
    let chord = binding.primary;
    let modifiers = if chord.shift {
        CMD_OR_CTRL | Modifiers::SHIFT
    } else {
        CMD_OR_CTRL
    };
    Some(Accelerator::new(Some(modifiers), key_code(chord.key)?))
}

/// egui key → muda key code, covering the chords in the binding table.
fn key_code(key: egui::Key) -> Option<Code> {
    Some(match key {
        egui::Key::A => Code::KeyA,
        egui::Key::G => Code::KeyG,
        egui::Key::K => Code::KeyK,
        egui::Key::S => Code::KeyS,
        egui::Key::Y => Code::KeyY,
        egui::Key::Z => Code::KeyZ,
        egui::Key::Comma => Code::Comma,
        _ => return None,
    })
}
