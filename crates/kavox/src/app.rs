//! Main `eframe::App` shell: coordinates core events, layout panels, overlays, drag-drop, and lifecycle.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui::{self, Frame, Key, Pos2, Stroke};
use kavox_core::{Controller, NoticeLevel, UiEvent};

use crate::hotkey::HotkeyService;
use crate::state::{AppCommand, UiState, View};
use crate::theme::{self, Palette};
use crate::tray::TrayHandle;
use crate::views::{self, ActionCtx};

/// Linux hides to the tray by closing the window and reopening it on demand: winit can't hide a
/// window on Wayland, and it also avoids X11's first-frame flash when starting minimized. The
/// tray (ksni) and global hotkeys run on their own threads, so nothing needs the event loop while
/// the window is closed. Windows keeps one window and toggles its visibility, since its tray needs
/// the Win32 message loop that winit runs.
pub const HIDE_BY_CLOSING: bool = cfg!(target_os = "linux");

pub struct KavoxApp {
    controller: Controller,
    hotkeys: HotkeyService,
    tray: Option<TrayHandle>,
    cmd_tx: Sender<AppCommand>,
    cmd_rx: Receiver<AppCommand>,
    state: UiState,
    last_dark_mode: Option<bool>,
    last_core_visible: bool,
    pending_initial_hide: bool,
    pending_paste_repaints: u8,
    shut_down: bool,
    /// The current window closed to hide in the tray rather than to quit.
    closed_to_tray: bool,
    /// Last inner size of the window, so a reopened window keeps it.
    window_size: Option<egui::Vec2>,
    _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

impl KavoxApp {
    pub fn new(
        controller: Controller,
        hotkeys: HotkeyService,
        cmd_tx: Sender<AppCommand>,
        cmd_rx: Receiver<AppCommand>,
        state: UiState,
        log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
    ) -> Self {
        let initial_visible = state.window_visible;
        Self {
            controller,
            hotkeys,
            tray: None,
            cmd_tx,
            cmd_rx,
            state,
            last_dark_mode: None,
            last_core_visible: initial_visible,
            pending_initial_hide: !initial_visible && !HIDE_BY_CLOSING,
            pending_paste_repaints: 0,
            shut_down: false,
            closed_to_tray: false,
            window_size: None,
            _log_guard: log_guard,
        }
    }

    /// Creates the tray once, on the main thread (required by tray-icon/muda on Windows).
    pub fn ensure_tray(&mut self) {
        if self.tray.is_none() {
            self.tray = Some(TrayHandle::create(
                self.controller.clone(),
                self.cmd_tx.clone(),
                self.state.settings.audio.mic_muted,
                self.state.settings.voice.enabled,
            ));
        }
    }

    /// Sets up a newly created window's egui context. Called for every window, since Linux
    /// opens a fresh one each time the app is shown from the tray.
    pub fn attach(&mut self, ctx: &egui::Context) {
        self.ensure_tray();
        theme::install_fonts(ctx);
        let palette = Palette::resolve(self.state.settings.app.theme, ctx);
        theme::apply_visuals(ctx, &palette);
        self.last_dark_mode = Some(palette.is_dark);
        self.closed_to_tray = false;
        self.pending_paste_repaints = 0;
    }

    /// Whether the window that just closed was hidden to the tray (the app keeps running).
    pub fn closed_to_tray(&self) -> bool {
        self.closed_to_tray && !self.state.quitting
    }

    pub fn window_visible(&self) -> bool {
        self.state.window_visible
    }

    pub fn window_size(&self) -> Option<egui::Vec2> {
        self.window_size
    }

    /// Runs while no window exists: applies core events and waits for the tray, IPC or a signal
    /// to show the window again. Returns `false` when the app should quit.
    pub fn wait_while_hidden(&mut self) -> bool {
        self.ensure_tray();
        self.state.window_visible = false;
        if self.last_core_visible {
            self.last_core_visible = false;
            self.controller.lock().set_visible(false);
        }
        while let Ok(cmd) = self.cmd_rx.recv() {
            match self.apply_command(cmd) {
                Some(Lifecycle::Show) => {
                    self.state.window_visible = true;
                    return true;
                }
                Some(Lifecycle::Quit) => {
                    self.state.quitting = true;
                    self.shutdown_once();
                    return false;
                }
                None => {}
            }
        }
        self.state.quitting = true;
        self.shutdown_once();
        false
    }

    pub fn shutdown_once(&mut self) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        tracing::info!("shutting down");
        crate::ipc::mark_shutting_down();
        if let Some(tray) = self.tray.as_mut() {
            tray.shutdown();
        }
        self.controller.shutdown();
    }

    fn sync_core_visibility(&mut self, ctx: &egui::Context) {
        let minimized = ctx.input(|i| i.viewport().minimized == Some(true));
        let core_visible = self.state.window_visible && !minimized;
        if core_visible != self.last_core_visible {
            self.last_core_visible = core_visible;
            self.controller.lock().set_visible(core_visible);
        }
    }

    /// Applies a background command; returns window lifecycle requests for the caller.
    fn apply_command(&mut self, cmd: AppCommand) -> Option<Lifecycle> {
        match cmd {
            AppCommand::CoreEvent(ev) => {
                if let UiEvent::Settings(settings) = &ev
                    && let Some(tray) = self.tray.as_mut()
                {
                    tray.sync_state(settings.audio.mic_muted, settings.voice.enabled);
                }
                self.state.apply_core_event(ev);
            }
            AppCommand::ImportFinished(result) => match result {
                Ok(done) => {
                    if done.added.len() == 1 {
                        self.state.push_toast(
                            NoticeLevel::Info,
                            format!("Added \"{}\"", done.added[0].name),
                        );
                    } else if done.added.len() > 1 {
                        self.state.push_toast(
                            NoticeLevel::Info,
                            format!("Added {} sounds", done.added.len()),
                        );
                    }
                    for failure in done.failed {
                        self.state.push_toast(
                            NoticeLevel::Error,
                            format!("{}: {}", failure.file, failure.error),
                        );
                    }
                }
                Err(err) => self.state.push_toast(NoticeLevel::Error, err),
            },
            AppCommand::DevicesRefreshed(devices) => {
                self.state.devices = devices;
                self.state.devices_rescanning = false;
            }
            AppCommand::ShowWindow => return Some(Lifecycle::Show),
            AppCommand::Quit => return Some(Lifecycle::Quit),
        }
        None
    }

    fn drain_commands(&mut self, ctx: &egui::Context) {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match self.apply_command(cmd) {
                Some(Lifecycle::Show) => {
                    self.pending_initial_hide = false;
                    self.state.window_visible = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    if !self.last_core_visible {
                        self.last_core_visible = true;
                        self.controller.lock().set_visible(true);
                    }
                }
                Some(Lifecycle::Quit) => {
                    self.state.quitting = true;
                    self.shutdown_once();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                None => {}
            }
        }
    }
}

enum Lifecycle {
    Show,
    Quit,
}

fn is_cut_shortcut(modifiers: egui::Modifiers, key: Key) -> bool {
    key == Key::Cut
        || (modifiers.command_only() && key == Key::X)
        || (cfg!(target_os = "windows")
            && modifiers.shift
            && !modifiers.ctrl
            && !modifiers.alt
            && !modifiers.mac_cmd
            && key == Key::Delete)
}

fn is_copy_shortcut(modifiers: egui::Modifiers, key: Key) -> bool {
    key == Key::Copy
        || (modifiers.command_only() && key == Key::C)
        || (cfg!(target_os = "windows")
            && modifiers.ctrl
            && !modifiers.shift
            && !modifiers.alt
            && !modifiers.mac_cmd
            && key == Key::Insert)
}

fn is_paste_shortcut(modifiers: egui::Modifiers, key: Key) -> bool {
    key == Key::Paste
        || (modifiers.command_only() && key == Key::V)
        || (cfg!(target_os = "windows")
            && modifiers.shift
            && !modifiers.ctrl
            && !modifiers.alt
            && !modifiers.mac_cmd
            && key == Key::Insert)
}

/// Lends the long-lived app to one eframe window, so Linux can close and reopen windows while
/// the app keeps its state. This is the only `eframe::App` impl: `KavoxApp`'s hooks are inherent
/// methods, so a hook can't be implemented on `KavoxApp` and silently never forwarded.
pub struct Hosted<'a>(pub &'a mut KavoxApp);

impl eframe::App for Hosted<'_> {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.0.raw_input_hook(ctx, raw_input);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.0.logic(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.0.ui(ui, frame);
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        self.0.on_exit(gl);
    }
}

/// Per-window `eframe` hooks, forwarded by [`Hosted`].
impl KavoxApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if raw_input
            .events
            .iter()
            .any(|e| matches!(e, egui::Event::Paste(_) | egui::Event::PasteImage(_)))
        {
            self.pending_paste_repaints = 0;
        }

        // While recording a hotkey, pass every key press through untouched so Ctrl+C/X/V
        // (even with an empty clipboard), Shift+Insert, Ctrl+Insert, and Shift+Delete all
        // arrive as exact `Event::Key` events.
        if self.state.recording_hotkey.is_some() {
            return;
        }

        let mut request_paste = false;
        let mut out = Vec::with_capacity(raw_input.events.len());
        for ev in raw_input.events.drain(..) {
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = ev
            {
                if is_cut_shortcut(modifiers, key) {
                    out.push(egui::Event::Cut);
                    continue;
                }
                if is_copy_shortcut(modifiers, key) {
                    out.push(egui::Event::Copy);
                    continue;
                }
                if is_paste_shortcut(modifiers, key) {
                    request_paste = true;
                    continue;
                }
            }
            out.push(ev);
        }
        raw_input.events = out;

        if request_paste {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            self.pending_paste_repaints = 2;
            ctx.request_repaint();
        }
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_commands(ctx);

        if self.pending_paste_repaints > 0 {
            self.pending_paste_repaints -= 1;
            ctx.request_repaint();
        }

        // Close-to-tray handling.
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.state.settings.app.close_to_tray && !self.state.quitting {
                self.state.window_visible = false;
                if HIDE_BY_CLOSING {
                    // Let the window close; `main` keeps the app running and reopens it later.
                    self.closed_to_tray = true;
                    self.window_size = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            } else {
                self.state.quitting = true;
                self.shutdown_once();
            }
        }

        // Pause core 30 Hz level metering while hidden in the tray OR minimized.
        self.sync_core_visibility(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // eframe's glow backend forces `window.set_visible(true)` in `post_rendering()` on the
        // first painted frame, ignoring `ViewportBuilder::with_visible(false)`. Queue
        // `ViewportCommand::Visible(false)` on that same frame so `handle_viewport_output()`
        // hides the window right after `post_rendering()` runs.
        if self.pending_initial_hide {
            self.pending_initial_hide = false;
            if !self.state.window_visible {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }

        // Resolve light/dark palette and sync egui visuals whenever the mode changes.
        let p = Palette::resolve(self.state.settings.app.theme, &ctx);
        if self.last_dark_mode != Some(p.is_dark) {
            theme::apply_visuals(&ctx, &p);
            self.last_dark_mode = Some(p.is_dark);
        }

        // Ctrl+F / Cmd+F focuses the search input on the Sounds view.
        if self.state.view == View::Sounds
            && self.state.dialog.is_none()
            && self.state.confirm.is_none()
            && self.state.recording_hotkey.is_none()
            && ctx.input(|i| i.modifiers.command && i.key_pressed(Key::F))
        {
            self.state.focus_search = true;
        }

        // OS file drop -> background import to the active board.
        let dropped_paths: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped_paths.is_empty() {
            let board = Some(self.state.library.active_board.clone());
            crate::state::spawn_import_paths(
                self.controller.clone(),
                board,
                dropped_paths,
                self.cmd_tx.clone(),
                ctx.clone(),
            );
        }

        let mut actions = ActionCtx {
            controller: &self.controller,
            hotkeys: &mut self.hotkeys,
            cmd_tx: &self.cmd_tx,
            egui_ctx: &ctx,
        };

        // 1. Top bar + optional status banner.
        egui::Panel::top("kavox-top-bar")
            .frame(Frame::NONE.fill(p.bg))
            .show_separator_line(false)
            .show(ui, |ui| {
                views::top_bar::show(ui, &p, &mut self.state, &mut actions);
                views::feedback::show_status_banner(ui, &p, &mut self.state, &mut actions);
            });

        // 2. Bottom mixer strip.
        egui::Panel::bottom("kavox-mixer-strip")
            .frame(Frame::NONE.fill(p.panel))
            .show_separator_line(false)
            .show(ui, |ui| {
                views::mixer_strip::show(ui, &p, &mut self.state, &mut actions);
            });

        // 3. Pad editor drawer (right side panel on wide windows, bottom sheet on narrow windows).
        let show_editor = self.state.view == View::Sounds && self.state.editing.is_some();
        if show_editor {
            let screen = ctx.content_rect();
            if screen.width() >= 720.0 {
                egui::Panel::right("kavox-pad-editor")
                    .exact_size(340.0)
                    .resizable(false)
                    .frame(Frame::NONE.fill(p.panel))
                    .show_separator_line(false)
                    .show(ui, |ui| {
                        let r = ui.max_rect();
                        ui.painter().line_segment(
                            [Pos2::new(r.min.x, r.min.y), Pos2::new(r.min.x, r.max.y)],
                            Stroke::new(1.0, p.line),
                        );
                        views::pad_editor::show(ui, &p, &mut self.state, &mut actions);
                    });
            } else {
                let sheet_h = (screen.height() * 0.56).clamp(240.0, 440.0);
                egui::Panel::bottom("kavox-pad-editor-sheet")
                    .exact_size(sheet_h)
                    .resizable(false)
                    .frame(Frame::NONE.fill(p.panel))
                    .show_separator_line(false)
                    .show(ui, |ui| {
                        let r = ui.max_rect();
                        ui.painter().line_segment(
                            [Pos2::new(r.min.x, r.min.y), Pos2::new(r.max.x, r.min.y)],
                            Stroke::new(1.0, p.line),
                        );
                        views::pad_editor::show(ui, &p, &mut self.state, &mut actions);
                    });
            }
        }

        // 4. Main central panel: Sounds pad grid or Voice changer board.
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(p.bg))
            .show(ui, |ui| match self.state.view {
                View::Sounds => views::pads::show(ui, &p, &mut self.state, &mut actions),
                View::Voice => views::voice_board::show(ui, &p, &mut self.state, &mut actions),
            });

        // 5. Modals & floating overlays.
        views::settings::show(&ctx, &p, &mut self.state, &mut actions);
        views::setup_wizard::show(&ctx, &p, &mut self.state, &mut actions);
        views::feedback::show_confirm_dialog(&ctx, &p, &mut self.state, &mut actions);
        views::feedback::show_toasts(&ctx, &p, &mut self.state);
        views::feedback::show_drop_overlay(&ctx, &p, &self.state);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if !self.closed_to_tray() {
            self.shutdown_once();
        }
    }
}

#[cfg(test)]
mod screenshots;
