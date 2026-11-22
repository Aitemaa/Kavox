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

pub struct KavoxApp {
    controller: Controller,
    hotkeys: HotkeyService,
    tray: TrayHandle,
    cmd_tx: Sender<AppCommand>,
    cmd_rx: Receiver<AppCommand>,
    state: UiState,
    last_dark_mode: Option<bool>,
    last_core_visible: bool,
    shut_down: bool,
    _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

impl KavoxApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        controller: Controller,
        hotkeys: HotkeyService,
        tray: TrayHandle,
        cmd_tx: Sender<AppCommand>,
        cmd_rx: Receiver<AppCommand>,
        state: UiState,
        log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
    ) -> Self {
        theme::install_fonts(&cc.egui_ctx);

        let palette = Palette::resolve(state.settings.app.theme, &cc.egui_ctx);
        theme::apply_visuals(&cc.egui_ctx, &palette);
        let initial_visible = state.window_visible;

        Self {
            controller,
            hotkeys,
            tray,
            cmd_tx,
            cmd_rx,
            state,
            last_dark_mode: Some(palette.is_dark),
            last_core_visible: initial_visible,
            shut_down: false,
            _log_guard: log_guard,
        }
    }

    fn shutdown_once(&mut self) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        tracing::info!("shutting down");
        self.tray.shutdown();
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

    fn drain_commands(&mut self, ctx: &egui::Context) {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                AppCommand::CoreEvent(ev) => {
                    if let UiEvent::Settings(settings) = &ev {
                        self.tray
                            .sync_state(settings.audio.mic_muted, settings.voice.enabled);
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
                AppCommand::ShowWindow => {
                    self.state.window_visible = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    if !self.last_core_visible {
                        self.last_core_visible = true;
                        self.controller.lock().set_visible(true);
                    }
                }
                AppCommand::Quit => {
                    self.state.quitting = true;
                    self.shutdown_once();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }
}

impl eframe::App for KavoxApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_commands(ctx);

        // Close-to-tray handling.
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.state.settings.app.close_to_tray && !self.state.quitting {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.state.window_visible = false;
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
        self.shutdown_once();
    }
}
