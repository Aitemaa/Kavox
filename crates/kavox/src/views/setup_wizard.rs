//! First-run SetupWizard modal matching `src/components/SetupWizard.tsx`.

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Key, Layout, Margin, Pos2, ScrollArea, Sense,
    Stroke, StrokeKind, Ui, Vec2,
};
use kavox_core::{StreamRole, VirtualStatus};

use crate::icons::{IconName, draw_logo_mark, paint_icon};
use crate::platform::VB_CABLE_URL;
use crate::state::{Dialog, UiState, mic_label};
use crate::theme::{Palette, font_body, font_heading, font_medium, mix};
use crate::views::{ActionCtx, settings::show_kavoxctl_guide};
use crate::widgets::{
    ButtonVariant, device_select, level_bar, link_button, modal, pill_button, switch_row,
};

const STEPS: &[&str] = &["Virtual microphone", "Your devices", "Test", "Voice apps"];

pub fn show(ctx: &egui::Context, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    if state.dialog != Some(Dialog::Wizard) {
        return;
    }

    let mut finish = false;
    if state.confirm.is_none() && ctx.input(|i| i.key_pressed(Key::Escape)) {
        finish = true;
    }

    let clicked_scrim = modal(ctx, p, "wizard-modal", 680.0, Some(540.0), |ui| {
        // Header: LogoMark + title + 4-step progress strip
        Frame::NONE
            .inner_margin(Margin {
                left: 22,
                right: 22,
                top: 20,
                bottom: 16,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    draw_logo_mark(ui, 32.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 10.0;
                        ui.label(
                            egui::RichText::new("Set up Kavox")
                                .font(font_heading(19.0))
                                .color(p.text),
                        );
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(18.0, 6.0);
                            for (i, &title) in STEPS.iter().enumerate() {
                                let is_cur = i == state.wizard_step;
                                let is_done = i < state.wizard_step;
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    let (num_rect, _) =
                                        ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                                    let painter = ui.painter();
                                    if is_cur {
                                        painter.circle_filled(num_rect.center(), 10.0, p.red);
                                        let g = painter.layout_no_wrap(
                                            (i + 1).to_string(),
                                            font_medium(11.0),
                                            Color32::WHITE,
                                        );
                                        painter.galley(
                                            num_rect.center() - g.size() * 0.5,
                                            g,
                                            Color32::WHITE,
                                        );
                                    } else if is_done {
                                        painter.circle_filled(num_rect.center(), 10.0, p.surface_2);
                                        paint_icon(
                                            painter,
                                            num_rect.center(),
                                            12.0,
                                            IconName::Check,
                                            p.text,
                                        );
                                    } else {
                                        painter.circle_stroke(
                                            num_rect.center(),
                                            9.5,
                                            Stroke::new(1.0, p.line_strong),
                                        );
                                        let g = painter.layout_no_wrap(
                                            (i + 1).to_string(),
                                            font_medium(11.0),
                                            p.muted,
                                        );
                                        painter.galley(
                                            num_rect.center() - g.size() * 0.5,
                                            g,
                                            p.muted,
                                        );
                                    }
                                    ui.label(
                                        egui::RichText::new(title)
                                            .font(font_body(13.0))
                                            .color(if is_cur { p.text } else { p.muted }),
                                    );
                                });
                            }
                        });
                    });
                });
            });

        let hr = ui.min_rect();
        ui.painter().line_segment(
            [Pos2::new(hr.min.x, hr.max.y), Pos2::new(hr.max.x, hr.max.y)],
            Stroke::new(1.0, p.line),
        );

        // Scrollable step body
        let footer_h = 65.0;
        let body_h = (ui.available_height() - footer_h).max(180.0);
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), body_h),
            Layout::top_down(Align::LEFT),
            |ui| {
                ScrollArea::vertical()
                    .id_salt(("wizard-body", state.wizard_step))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        Frame::NONE
                            .inner_margin(Margin::symmetric(26, 24))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width().min(580.0));
                                ui.spacing_mut().item_spacing.y = 14.0;
                                match state.wizard_step {
                                    0 => show_cable_step(ui, p, state, actions),
                                    1 => show_devices_step(ui, p, state, actions),
                                    2 => show_test_step(ui, p, state, actions),
                                    _ => show_apps_step(ui, p, state),
                                }
                            });
                    });
            },
        );

        // Footer divider + navigation buttons
        let f_top = ui.cursor().top();
        let f_rect = ui.max_rect();
        ui.painter().line_segment(
            [
                Pos2::new(f_rect.min.x, f_top),
                Pos2::new(f_rect.max.x, f_top),
            ],
            Stroke::new(1.0, p.line),
        );
        Frame::NONE
            .inner_margin(Margin::symmetric(22, 14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let is_last = state.wizard_step + 1 >= STEPS.len();
                    if pill_button(
                        ui,
                        p,
                        if is_last { "Close" } else { "Skip setup" },
                        None,
                        ButtonVariant::Quiet,
                        true,
                    )
                    .clicked()
                    {
                        finish = true;
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if is_last {
                            if pill_button(ui, p, "Finish", None, ButtonVariant::Primary, true)
                                .clicked()
                            {
                                finish = true;
                            }
                        } else if pill_button(ui, p, "Next", None, ButtonVariant::Primary, true)
                            .clicked()
                        {
                            state.wizard_step = (state.wizard_step + 1).min(STEPS.len() - 1);
                        }

                        if state.wizard_step > 0
                            && pill_button(ui, p, "Back", None, ButtonVariant::Default, true)
                                .clicked()
                        {
                            state.wizard_step -= 1;
                        }
                    });
                });
            });
    });

    if finish || clicked_scrim {
        state.dialog = None;
        state.wizard_step = 0;
        state.update_app(actions.controller, |a| a.setup_complete = true);
    }
}

#[derive(Clone, Copy)]
enum LineTone {
    Ok,
    Warn,
    Bad,
}

fn status_line(ui: &mut Ui, p: &Palette, tone: LineTone, text: &str) {
    let (bg, icon, icon_col) = match tone {
        LineTone::Ok => (p.surface, IconName::Check, p.text),
        LineTone::Warn => (mix(p.amber, p.panel, 0.12), IconName::Warning, p.amber),
        LineTone::Bad => (mix(p.red, p.panel, 0.10), IconName::Warning, p.red_text),
    };
    Frame::NONE
        .fill(bg)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let (ir, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                paint_icon(ui.painter(), ir.center(), 18.0, icon, icon_col);
                ui.label(
                    egui::RichText::new(text)
                        .font(font_body(14.0))
                        .color(p.text),
                );
            });
        });
}

fn show_cable_step(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    if state.platform != "windows" {
        ui.label(
            egui::RichText::new("Kavox makes its own microphone")
                .font(font_heading(18.0))
                .color(p.text),
        );
        ui.label(
            egui::RichText::new("While Kavox runs, your system has an extra input called Kavox Mic. Voice apps that use it hear your sounds mixed with your voice. It disappears when you quit Kavox.")
                .font(font_body(14.0))
                .color(p.text),
        );
        match &state.virtual_status {
            VirtualStatus::Failed { message } => {
                status_line(
                    ui,
                    p,
                    LineTone::Bad,
                    &format!("{message}. Kavox needs PulseAudio, or PipeWire with pipewire-pulse."),
                );
            }
            _ => status_line(ui, p, LineTone::Ok, "Kavox Mic is ready."),
        }
        return;
    }

    ui.label(
        egui::RichText::new("Give Kavox a way into your voice apps")
            .font(font_heading(18.0))
            .color(p.text),
    );
    ui.label(
        egui::RichText::new("Windows apps can't create microphones on their own, so Kavox plays into a free virtual cable called VB-CABLE. Discord and games then pick the cable's other end as their microphone.")
            .font(font_body(14.0))
            .color(p.text),
    );

    let status = state.virtual_status.clone();
    match &status {
        VirtualStatus::NotInstalled => {
            status_line(ui, p, LineTone::Bad, "No virtual cable found yet.");
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(egui::RichText::new("1.").font(font_body(14.0)).color(p.muted));
                    if link_button(ui, p, "Download VB-CABLE").clicked() {
                        let res = crate::platform::open_url(VB_CABLE_URL);
                        actions.run_or_toast(state, res);
                    }
                    ui.label(
                        egui::RichText::new("and unzip it.")
                            .font(font_body(14.0))
                            .color(p.text),
                    );
                });
                numbered_item(
                    ui,
                    p,
                    "2.",
                    "Right-click VBCABLE_Setup_x64.exe, choose Run as administrator, then click Install Driver.",
                );
                numbered_item(ui, p, "3.", "Restart your PC if the installer asks you to.");
            });
        }
        VirtualStatus::CableIsDefaultOutput { device, .. } => {
            status_line(
                ui,
                p,
                LineTone::Warn,
                &format!(
                    "Found {device}, but it became your default speaker. Everything your PC plays would go into your mic."
                ),
            );
            ui.label(
                egui::RichText::new(
                    "Open Sound settings and choose your real speakers or headphones as the output device.",
                )
                .font(font_body(14.0))
                .color(p.text),
            );
            if pill_button(
                ui,
                p,
                "Open Sound settings",
                Some(IconName::External),
                ButtonVariant::Default,
                true,
            )
            .clicked()
            {
                let res = crate::platform::open_sound_settings();
                actions.run_or_toast(state, res);
            }
        }
        VirtualStatus::Ok { device, .. } => {
            status_line(ui, p, LineTone::Ok, &format!("Found {device}. You're set."));
        }
        VirtualStatus::Missing { device } => {
            status_line(ui, p, LineTone::Bad, &format!("{device} isn't connected."));
        }
        VirtualStatus::Failed { message } => {
            status_line(ui, p, LineTone::Bad, message);
        }
    }

    if !matches!(status, VirtualStatus::Ok { .. })
        && pill_button(
            ui,
            p,
            if state.wizard_checking {
                "Checking…"
            } else {
                "Check again"
            },
            None,
            ButtonVariant::Default,
            !state.wizard_checking,
        )
        .clicked()
    {
        state.wizard_checking = true;
        let fresh = actions.controller.lock().devices();
        state.devices = fresh;
        state.wizard_checking = false;
    }
}

fn show_devices_step(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    let audio = state.settings.audio.clone();
    let devices = state.devices.clone();

    ui.label(
        egui::RichText::new("Pick your mic and headphones")
            .font(font_heading(18.0))
            .color(p.text),
    );

    if let Some(next) = device_select(
        ui,
        p,
        "wizard-mic",
        "Microphone",
        audio.mic_device.as_deref(),
        &devices.inputs,
        "System default",
    ) {
        state.update_audio(actions.controller, |a| a.mic_device = next);
    }

    level_bar(
        ui,
        p,
        ui.make_persistent_id("wizard-mic-level"),
        state.levels.mic,
    );
    ui.label(
        egui::RichText::new("Say something; the bar should move.")
            .font(font_body(13.0))
            .color(p.muted),
    );

    if let Some(next) = switch_row(
        ui,
        p,
        "wizard-mic-passthrough",
        "Send my voice through Kavox",
        Some("Leave this on so friends hear you and your sounds through one microphone."),
        audio.mic_passthrough,
    ) {
        state.update_audio(actions.controller, |a| a.mic_passthrough = next);
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
        "wizard-monitor",
        "Headphones or speakers",
        audio.monitor_output.as_deref(),
        &non_virtual_outputs,
        "System default",
    ) {
        state.update_audio(actions.controller, |a| a.monitor_output = next);
    }

    if pill_button(
        ui,
        p,
        "Play test tone in headphones",
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
}

fn show_test_step(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    let label = mic_label(&state.virtual_status).map(str::to_owned);

    ui.label(
        egui::RichText::new("Send a test tone into the mic")
            .font(font_heading(18.0))
            .color(p.text),
    );
    ui.label(
        egui::RichText::new(format!(
            "This plays a short tone into {}, the way your sounds will reach voice apps. You won't hear it unless an app is listening to that mic.",
            label.as_deref().unwrap_or("the virtual microphone")
        ))
        .font(font_body(14.0))
        .color(p.text),
    );

    if pill_button(
        ui,
        p,
        "Play test tone into mic",
        Some(IconName::Broadcast),
        ButtonVariant::Primary,
        label.is_some(),
    )
    .clicked()
    {
        let res = actions
            .controller
            .lock()
            .play_test_tone(StreamRole::Virtual);
        actions.run_or_toast(state, res);
    }

    level_bar(
        ui,
        p,
        ui.make_persistent_id("wizard-virtual-level"),
        state.levels.virtual_out,
    );

    if label.is_none() {
        status_line(
            ui,
            p,
            LineTone::Bad,
            "Finish the first step so Kavox has a microphone to play into.",
        );
    }

    ui.label(
        egui::RichText::new(
            "To check from the other end, use the mic test in Discord's Voice & Video settings.",
        )
        .font(font_body(13.0))
        .color(p.muted),
    );
}

fn show_apps_step(ui: &mut Ui, p: &Palette, state: &mut UiState) {
    let label = mic_label(&state.virtual_status)
        .unwrap_or("the Kavox microphone")
        .to_owned();

    ui.label(
        egui::RichText::new("Point your voice apps at Kavox")
            .font(font_heading(18.0))
            .color(p.text),
    );

    // Discord card
    app_card(ui, p, "Discord", |ui| {
        numbered_item(
            ui,
            p,
            "1.",
            &format!("Open User Settings, then Voice & Video, and set Input Device to {label}."),
        );
        numbered_item(
            ui,
            p,
            "2.",
            "Set Noise Suppression to None. Otherwise Discord filters out anything that isn't a voice.",
        );
        numbered_item(
            ui,
            p,
            "3.",
            "Turn off Echo Cancellation and Automatic Gain Control.",
        );
        numbered_item(
            ui,
            p,
            "4.",
            "If you use voice activity, set the sensitivity by hand and keep it low so quiet sounds get through.",
        );
    });

    // OBS & games card
    app_card(ui, p, "OBS and games", |ui| {
        ui.label(
            egui::RichText::new(format!(
                "Choose {label} wherever the app asks for a microphone. In OBS, add an Audio Input Capture source with that device."
            ))
            .font(font_body(14.0))
            .color(p.text),
        );
    });

    if !state.hotkeys_supported {
        show_kavoxctl_guide(ui, p, state);
    }
}

fn app_card(ui: &mut Ui, p: &Palette, title: &str, body: impl FnOnce(&mut Ui)) {
    Frame::NONE
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(
                egui::RichText::new(title)
                    .font(font_heading(15.0))
                    .color(p.text),
            );
            body(ui);
        });
}

fn numbered_item(ui: &mut Ui, p: &Palette, num: &str, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(
            egui::RichText::new(num)
                .font(font_body(14.0))
                .color(p.muted),
        );
        ui.label(
            egui::RichText::new(text)
                .font(font_body(14.0))
                .color(p.text),
        );
    });
}

#[allow(dead_code)]
fn _keep_stroke_kind(_: StrokeKind) {}
