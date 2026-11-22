//! Settings modal dialog matching `src/components/Settings.tsx` (Audio, Hotkeys, App, About + KavoxctlGuide).

use eframe::egui::{
    self, Align, Color32, CornerRadius, CursorIcon, Frame, Key, Layout, Margin, Pos2, ScrollArea,
    Sense, Stroke, StrokeKind, Ui, Vec2,
};
use kavox_core::{GlobalHotkey, NoticeLevel, StreamRole, Theme, VirtualStatus};

use crate::icons::{IconName, draw_logo_mark};
use crate::platform::{REPO_URL, VB_CABLE_URL};
use crate::state::{Compositor, Dialog, SettingsTab, UiState, mic_label};
use crate::theme::{Palette, font_body, font_heading, font_medium, font_mono};
use crate::views::ActionCtx;
use crate::widgets::{
    ButtonVariant, callout, device_select, hotkey_recorder, icon_button, link_button, modal,
    pill_button, segmented, switch_row,
};

const TABS: &[(SettingsTab, &str)] = &[
    (SettingsTab::Audio, "Audio"),
    (SettingsTab::Hotkeys, "Hotkeys"),
    (SettingsTab::App, "App"),
    (SettingsTab::About, "About"),
];

pub fn show(ctx: &egui::Context, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    if state.dialog != Some(Dialog::Settings) {
        return;
    }

    if state.confirm.is_none()
        && state.recording_hotkey.is_none()
        && ctx.input(|i| i.key_pressed(Key::Escape))
    {
        state.dialog = None;
        return;
    }

    let mut close_requested = false;
    let clicked_scrim = modal(ctx, p, "settings-modal", 840.0, Some(660.0), |ui| {
        // Header
        Frame::NONE
            .inner_margin(Margin {
                left: 22,
                right: 12,
                top: 14,
                bottom: 14,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Settings")
                            .font(font_heading(17.0))
                            .color(p.text),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, p, IconName::Close, 18.0, 34.0, "Close settings")
                            .clicked()
                        {
                            close_requested = true;
                        }
                    });
                });
            });

        let hr = ui.min_rect();
        ui.painter().line_segment(
            [Pos2::new(hr.min.x, hr.max.y), Pos2::new(hr.max.x, hr.max.y)],
            Stroke::new(1.0, p.line),
        );

        let body_h = ui.available_height().max(200.0);
        let total_w = ui.available_width();
        let narrow = total_w < 620.0;

        if narrow {
            // Horizontal tabs bar on narrow windows
            Frame::NONE
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for &(tab, label) in TABS {
                            if tab_button(ui, p, label, state.settings_tab == tab, false).clicked()
                            {
                                state.settings_tab = tab;
                            }
                        }
                    });
                });
            let tr = ui.min_rect();
            ui.painter().line_segment(
                [Pos2::new(tr.min.x, tr.max.y), Pos2::new(tr.max.x, tr.max.y)],
                Stroke::new(1.0, p.line),
            );
            show_tab_panel(ui, p, state, actions, true);
        } else {
            let sidebar_w = 180.0;
            let panel_w = (total_w - sidebar_w).max(240.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;

                // Left sidebar
                let sidebar_rect = ui
                    .allocate_ui_with_layout(
                        Vec2::new(sidebar_w, body_h),
                        Layout::top_down(Align::LEFT),
                        |ui| {
                            Frame::NONE
                                .inner_margin(Margin::symmetric(10, 14))
                                .show(ui, |ui| {
                                    ui.set_width(sidebar_w - 20.0);
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    for &(tab, label) in TABS {
                                        if tab_button(ui, p, label, state.settings_tab == tab, true)
                                            .clicked()
                                        {
                                            state.settings_tab = tab;
                                        }
                                    }
                                });
                        },
                    )
                    .response
                    .rect;

                // Vertical divider
                ui.painter().line_segment(
                    [
                        Pos2::new(sidebar_rect.max.x, sidebar_rect.min.y),
                        Pos2::new(sidebar_rect.max.x, sidebar_rect.min.y + body_h),
                    ],
                    Stroke::new(1.0, p.line),
                );

                // Right scrollable panel
                ui.allocate_ui_with_layout(
                    Vec2::new(panel_w, body_h),
                    Layout::top_down(Align::LEFT),
                    |ui| show_tab_panel(ui, p, state, actions, false),
                );
            });
        }
    });

    if close_requested || clicked_scrim {
        state.dialog = None;
        state.recording_hotkey = None;
    }
}

fn tab_button(
    ui: &mut Ui,
    p: &Palette,
    label: &str,
    active: bool,
    full_width: bool,
) -> egui::Response {
    let font = font_medium(14.0);
    let g = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), p.text);
    let w = if full_width {
        ui.available_width()
    } else {
        12.0 + 6.0 + 10.0 + g.size().x + 14.0
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 34.0), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let cy = rect.center().y;
        if active {
            painter.circle_filled(Pos2::new(rect.min.x + 15.0, cy), 3.0, p.red);
        }
        let fg = if active || resp.hovered() {
            p.text
        } else {
            p.muted
        };
        let tg = painter.layout_no_wrap(label.to_owned(), font, fg);
        painter.galley(Pos2::new(rect.min.x + 28.0, cy - tg.size().y * 0.5), tg, fg);
    }
    resp
}

fn show_tab_panel(
    ui: &mut Ui,
    p: &Palette,
    state: &mut UiState,
    actions: &mut ActionCtx<'_>,
    narrow: bool,
) {
    let margin = if narrow {
        Margin::same(16)
    } else {
        Margin {
            left: 26,
            right: 26,
            top: 22,
            bottom: 30,
        }
    };
    ScrollArea::vertical()
        .id_salt(("settings-panel", state.settings_tab as u8))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            Frame::NONE.inner_margin(margin).show(ui, |ui| {
                let w = ui.available_width().min(560.0);
                ui.set_width(w);
                ui.spacing_mut().item_spacing.y = 30.0;
                match state.settings_tab {
                    SettingsTab::Audio => show_audio_tab(ui, p, state, actions),
                    SettingsTab::Hotkeys => show_hotkeys_tab(ui, p, state, actions),
                    SettingsTab::App => show_app_tab(ui, p, state, actions),
                    SettingsTab::About => show_about_tab(ui, p, state, actions),
                }
            });
        });
}

fn show_audio_tab(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    let audio = state.settings.audio.clone();
    let devices = state.devices.clone();
    let label = mic_label(&state.virtual_status).map(str::to_owned);

    // Group 1: Where your sounds go
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 14.0;
        ui.label(
            egui::RichText::new("Where your sounds go")
                .font(font_heading(15.0))
                .color(p.text),
        );

        if state.platform == "windows" {
            let default_label = if devices.virtual_candidates.is_empty() {
                "Detect automatically (none found)"
            } else {
                "Detect automatically"
            };
            if let Some(next) = device_select(
                ui,
                p,
                "settings-virtual-cable",
                "Virtual cable",
                audio.virtual_output.as_deref(),
                &devices.outputs,
                default_label,
            ) {
                state.update_audio(actions.controller, |a| a.virtual_output = next);
            }
            if devices.virtual_candidates.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(
                        egui::RichText::new("No virtual cable found.")
                            .font(font_body(13.0))
                            .color(p.muted),
                    );
                    if link_button(ui, p, "Get VB-CABLE (free)").clicked() {
                        let res = crate::platform::open_url(VB_CABLE_URL);
                        actions.run_or_toast(state, res);
                    }
                });
            } else {
                ui.label(
                    egui::RichText::new(
                        "Kavox plays into this device. Its other end shows up as a microphone in voice apps.",
                    )
                    .font(font_body(13.0))
                    .color(p.muted),
                );
            }
        } else {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(
                    egui::RichText::new("Virtual microphone")
                        .font(font_medium(13.0))
                        .color(p.text),
                );
                let msg = match &state.virtual_status {
                    VirtualStatus::Failed { message } => message.as_str(),
                    _ => "Kavox Mic, created while Kavox runs",
                };
                ui.label(egui::RichText::new(msg).font(font_body(14.0)).color(p.text));
            });
        }

        if let Some(mic) = &label {
            callout(ui, p, false, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(
                        egui::RichText::new("In Discord, OBS or your game, set the microphone to")
                            .font(font_body(14.0))
                            .color(p.text),
                    );
                    ui.label(
                        egui::RichText::new(format!("{mic}."))
                            .font(font_medium(14.0))
                            .color(p.text),
                    );
                });
            });
        }
    });

    // Group 2: Your microphone
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 14.0;
        ui.label(
            egui::RichText::new("Your microphone")
                .font(font_heading(15.0))
                .color(p.text),
        );
        if let Some(next) = switch_row(
            ui,
            p,
            "settings-mic-passthrough",
            "Send my voice through Kavox",
            Some("Mixes your real mic in with the sounds. Turn off if you only want the sounds, or mix your voice elsewhere."),
            audio.mic_passthrough,
        ) {
            state.update_audio(actions.controller, |a| a.mic_passthrough = next);
        }
        if let Some(next) = device_select(
            ui,
            p,
            "settings-mic-device",
            "Microphone",
            audio.mic_device.as_deref(),
            &devices.inputs,
            "System default",
        ) {
            state.update_audio(actions.controller, |a| a.mic_device = next);
        }
    });

    // Group 3: Hearing your sounds
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 14.0;
        ui.label(
            egui::RichText::new("Hearing your sounds")
                .font(font_heading(15.0))
                .color(p.text),
        );
        if let Some(next) = switch_row(
            ui,
            p,
            "settings-monitor-enabled",
            "Play sounds in my headphones too",
            None,
            audio.monitor_enabled,
        ) {
            state.update_audio(actions.controller, |a| a.monitor_enabled = next);
        }
        let non_virtual_outputs: Vec<_> = devices
            .outputs
            .iter()
            .filter(|d| !devices.virtual_candidates.contains(&d.id))
            .cloned()
            .collect();
        if let Some(next) = device_select(
            ui,
            p,
            "settings-monitor-output",
            "Headphones or speakers",
            audio.monitor_output.as_deref(),
            &non_virtual_outputs,
            "System default",
        ) {
            state.update_audio(actions.controller, |a| a.monitor_output = next);
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(8.0);
            if pill_button(
                ui,
                p,
                "Play test tone",
                Some(IconName::Headphones),
                ButtonVariant::Default,
                true,
            )
            .clicked()
            {
                let res = actions
                    .controller
                    .lock()
                    .play_test_tone(StreamRole::Monitor);
                actions.run_or_toast(state, res);
            }
            if pill_button(
                ui,
                p,
                if state.devices_rescanning {
                    "Rescanning…"
                } else {
                    "Rescan devices"
                },
                None,
                ButtonVariant::Default,
                !state.devices_rescanning,
            )
            .clicked()
            {
                actions.rescan_devices(state);
            }
        });
    });

    // Group 4: Volume
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 14.0;
        ui.label(
            egui::RichText::new("Volume")
                .font(font_heading(15.0))
                .color(p.text),
        );
        if let Some(next) = switch_row(
            ui,
            p,
            "settings-normalize",
            "Even out loudness",
            Some("Brings quiet and loud files to a similar level when they play. Each sound's own volume still applies."),
            audio.normalize,
        ) {
            state.update_audio(actions.controller, |a| a.normalize = next);
        }
        if let Some(next) = switch_row(
            ui,
            p,
            "settings-global-volume-override",
            "Use one volume for every sound",
            Some("Ignores each sound's own volume. An “All sounds” slider appears next to your mic at the bottom and sets the volume of every sound, in your headphones and in the mic. Your microphone isn't affected."),
            audio.global_volume_override,
        ) {
            state.update_audio(actions.controller, |a| a.global_volume_override = next);
        }
    });

    if pill_button(ui, p, "Run setup again", None, ButtonVariant::Default, true).clicked() {
        state.wizard_step = 0;
        state.dialog = Some(Dialog::Wizard);
    }
}

fn show_hotkeys_tab(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    if !state.hotkeys_supported {
        callout(ui, p, true, |ui| {
            ui.label(
                egui::RichText::new("You're on Wayland, where apps can't register global hotkeys. Kavox saves the keys below, and you can bind kavoxctl commands in your compositor instead.")
                    .font(font_body(14.0))
                    .color(p.text),
            );
        });
    }

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 14.0;
        ui.label(
            egui::RichText::new("Global hotkeys")
                .font(font_heading(15.0))
                .color(p.text),
        );
        ui.label(
            egui::RichText::new("Set a hotkey for each sound in its editor (right-click a pad).")
                .font(font_body(13.0))
                .color(p.muted),
        );

        let stop_all = state.settings.hotkeys.stop_all.clone();
        if let Some(next) = hotkey_recorder(
            ui,
            p,
            state,
            "global-stop-all",
            "Stop all sounds",
            stop_all.as_deref(),
        ) {
            match actions.set_global_hotkey(GlobalHotkey::StopAll, next) {
                Ok(()) => {
                    state.hotkey_errors.remove("global-stop-all");
                }
                Err(err) => {
                    state.hotkey_errors.insert("global-stop-all".into(), err);
                }
            }
        }

        let toggle_mute = state.settings.hotkeys.toggle_mute.clone();
        if let Some(next) = hotkey_recorder(
            ui,
            p,
            state,
            "global-toggle-mute",
            "Mute or unmute mic",
            toggle_mute.as_deref(),
        ) {
            match actions.set_global_hotkey(GlobalHotkey::ToggleMute, next) {
                Ok(()) => {
                    state.hotkey_errors.remove("global-toggle-mute");
                }
                Err(err) => {
                    state.hotkey_errors.insert("global-toggle-mute".into(), err);
                }
            }
        }
    });

    show_kavoxctl_guide(ui, p, state);
}

pub fn show_kavoxctl_guide(ui: &mut Ui, p: &Palette, state: &mut UiState) {
    let example = state
        .library
        .sounds
        .values()
        .next()
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "airhorn".into());
    let quoted = if example.contains(' ') {
        format!("\"{example}\"")
    } else {
        example.clone()
    };

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 14.0;
        ui.label(
            egui::RichText::new("Control Kavox from anywhere")
                .font(font_heading(15.0))
                .color(p.text),
        );
        ui.label(
            egui::RichText::new("kavoxctl talks to the running app, so any key binding, script or stream deck button can play sounds. It matches names exactly first, then by the start of the name.")
                .font(font_body(13.0))
                .color(p.muted),
        );

        if let Some(next) = segmented(
            ui,
            p,
            state.compositor,
            &[
                (Compositor::Niri, "niri", None),
                (Compositor::Hyprland, "Hyprland", None),
                (Compositor::Sway, "Sway", None),
                (Compositor::Other, "KDE, GNOME", None),
            ],
        ) {
            state.compositor = next;
        }

        let snippet = match state.compositor {
            Compositor::Niri => format!(
                "// ~/.config/niri/config.kdl, inside binds {{ }}\nMod+F1 {{ spawn \"kavoxctl\" \"play\" \"{example}\"; }}\nMod+F12 {{ spawn \"kavoxctl\" \"stop-all\"; }}"
            ),
            Compositor::Hyprland => format!(
                "# ~/.config/hypr/hyprland.conf\nbind = , F13, exec, kavoxctl play {quoted}\nbind = , F24, exec, kavoxctl stop-all"
            ),
            Compositor::Sway => format!(
                "# ~/.config/sway/config\nbindsym F13 exec kavoxctl play {quoted}\nbindsym F24 exec kavoxctl stop-all"
            ),
            Compositor::Other => format!(
                "# KDE: System Settings → Shortcuts → Add Command\n# GNOME: Settings → Keyboard → Custom Shortcuts\nkavoxctl play {quoted}"
            ),
        };

        Frame::NONE
            .fill(p.bg)
            .stroke(Stroke::new(1.0, p.line))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin {
                left: 14,
                right: 10,
                top: 10,
                bottom: 10,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    let code_w = (ui.available_width() - 40.0).max(120.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(code_w, 0.0),
                        Layout::top_down(Align::LEFT),
                        |ui| {
                            ScrollArea::horizontal()
                                .id_salt(("code-block", state.compositor as u8))
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&snippet)
                                                .font(font_mono(12.5))
                                                .color(p.text),
                                        )
                                        .selectable(true),
                                    );
                                });
                        },
                    );
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        if icon_button(ui, p, IconName::Copy, 16.0, 32.0, "Copy").clicked() {
                            ui.ctx().copy_text(snippet.clone());
                            state.push_toast(NoticeLevel::Info, "Copied");
                        }
                    });
                });
            });

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(egui::RichText::new("Also:").font(font_body(13.0)).color(p.muted));
            code_pill(ui, p, &format!("kavoxctl stop {quoted}"));
            ui.label(egui::RichText::new(",").font(font_body(13.0)).color(p.muted));
            code_pill(ui, p, "kavoxctl mute");
            ui.label(egui::RichText::new("(toggles),").font(font_body(13.0)).color(p.muted));
            code_pill(ui, p, "kavoxctl mute on");
            ui.label(egui::RichText::new(",").font(font_body(13.0)).color(p.muted));
            code_pill(ui, p, "kavoxctl list");
            ui.label(egui::RichText::new(".").font(font_body(13.0)).color(p.muted));
        });
    });
}

fn code_pill(ui: &mut Ui, p: &Palette, text: &str) {
    Frame::NONE
        .fill(p.surface)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .font(font_mono(12.0))
                    .color(p.text),
            );
        });
}

fn show_app_tab(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    let app = state.settings.app.clone();

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 14.0;
        ui.label(
            egui::RichText::new("Window")
                .font(font_heading(15.0))
                .color(p.text),
        );
        if let Some(next) = switch_row(
            ui,
            p,
            "app-close-to-tray",
            "Keep running when I close the window",
            Some("Kavox stays in the tray so hotkeys keep working. Open it again from the tray or by launching Kavox."),
            app.close_to_tray,
        ) {
            state.update_app(actions.controller, |a| a.close_to_tray = next);
        }
        if let Some(next) = switch_row(
            ui,
            p,
            "app-start-minimized",
            "Start hidden in the tray",
            None,
            app.start_minimized,
        ) {
            state.update_app(actions.controller, |a| a.start_minimized = next);
        }
        if let Some(next) = switch_row(
            ui,
            p,
            "app-launch-on-login",
            "Start Kavox when I log in",
            None,
            state.autostart_enabled,
        ) {
            state.set_launch_on_login(actions.controller, next);
        }
    });

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 14.0;
        ui.label(
            egui::RichText::new("Appearance")
                .font(font_heading(15.0))
                .color(p.text),
        );
        if let Some(next) = segmented(
            ui,
            p,
            app.theme,
            &[
                (Theme::System, "Match system", None),
                (Theme::Dark, "Dark", None),
                (Theme::Light, "Light", None),
            ],
        ) {
            state.update_app(actions.controller, |a| a.theme = next);
        }
    });
}

fn show_about_tab(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    let plat_label = match state.platform {
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        draw_logo_mark(ui, 48.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(
                egui::RichText::new("Kavox")
                    .font(font_heading(22.0))
                    .color(p.text),
            );
            ui.label(
                egui::RichText::new(format!(
                    "Version {} for {plat_label}. Free, no account, no tracking.",
                    state.version
                ))
                .font(font_body(13.0))
                .color(p.muted),
            );
            ui.label(
                egui::RichText::new(
                    "Open source under the GPL-3.0 license, provided without any warranty.",
                )
                .font(font_body(13.0))
                .color(p.muted),
            );
        });
    });

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(8.0);
        if pill_button(
            ui,
            p,
            "Copy diagnostics",
            Some(IconName::Copy),
            ButtonVariant::Default,
            true,
        )
        .clicked()
        {
            let text = actions.controller.lock().diagnostics(state.version);
            ui.ctx().copy_text(text);
            state.push_toast(
                NoticeLevel::Info,
                "Diagnostics copied. Paste them into your bug report.",
            );
        }
        if pill_button(
            ui,
            p,
            "Open logs folder",
            Some(IconName::Folder),
            ButtonVariant::Default,
            true,
        )
        .clicked()
        {
            let res = crate::platform::open_logs_folder(&state.log_dir);
            actions.run_or_toast(state, res);
        }
    });

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(8.0);
        if pill_button(
            ui,
            p,
            "Source code and issues",
            Some(IconName::External),
            ButtonVariant::Default,
            true,
        )
        .clicked()
        {
            let res = crate::platform::open_url(REPO_URL);
            actions.run_or_toast(state, res);
        }
        if state.platform == "windows"
            && pill_button(
                ui,
                p,
                "VB-CABLE website",
                Some(IconName::External),
                ButtonVariant::Default,
                true,
            )
            .clicked()
        {
            let res = crate::platform::open_url(VB_CABLE_URL);
            actions.run_or_toast(state, res);
        }
    });
}

#[allow(dead_code)]
fn _keep_imports(_: Color32, _: StrokeKind) {}
