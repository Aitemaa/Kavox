//! Right-side sound editor drawer matching `src/components/PadEditor.tsx`.

use eframe::egui::{
    self, Align, CursorIcon, Frame, Key, Layout, Margin, Pos2, ScrollArea, Sense, Stroke, Ui, Vec2,
};
use kavox_core::{Retrigger, SoundPatch, library::PALETTE};

use crate::icons::IconName;
use crate::state::{UiState, format_duration};
use crate::theme::{Palette, font_body, font_heading, font_medium, font_mono, parse_hex};
use crate::views::ActionCtx;
use crate::widgets::{
    ButtonVariant, fader, hotkey_recorder, icon_button, pill_button, segmented, switch_row,
    text_input,
};

fn mode_hint(mode: Retrigger) -> &'static str {
    match mode {
        Retrigger::Restart => "Pressing again starts the sound over.",
        Retrigger::Overlap => "Pressing again plays another copy on top.",
        Retrigger::Toggle => "Pressing again stops it.",
    }
}

pub fn show(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    let Some(sound_id) = state.editing.clone() else {
        return;
    };
    let Some(sound) = state.library.sounds.get(&sound_id).cloned() else {
        state.editing = None;
        return;
    };

    if state.editor_sound_id.as_deref() != Some(sound.id.as_str()) {
        state.editor_name_draft = sound.name.clone();
        state.editor_sound_id = Some(sound.id.clone());
    }

    // Escape closes the drawer when no dialog/confirm/hotkey recorder is active.
    if state.dialog.is_none()
        && state.confirm.is_none()
        && state.recording_hotkey.is_none()
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(Key::Escape))
    {
        state.editing = None;
        return;
    }

    let pad_color = parse_hex(&sound.color);
    let overridden = state.settings.audio.global_volume_override;

    // Header
    Frame::NONE
        .fill(p.panel)
        .inner_margin(Margin {
            left: 18,
            right: 10,
            top: 12,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                ui.painter()
                    .circle_filled(dot_rect.center(), 5.0, pad_color);
                ui.label(
                    egui::RichText::new("Edit sound")
                        .font(font_heading(15.0))
                        .color(p.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, p, IconName::Close, 18.0, 34.0, "Close").clicked() {
                        state.editing = None;
                    }
                });
            });
        });

    let h_rect = ui.min_rect();
    ui.painter().line_segment(
        [
            Pos2::new(h_rect.min.x, h_rect.max.y),
            Pos2::new(h_rect.max.x, h_rect.max.y),
        ],
        Stroke::new(1.0, p.line),
    );

    // Reserve footer height (~62px)
    let footer_h = 62.0;
    let body_h = (ui.available_height() - footer_h).max(120.0);

    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), body_h),
        Layout::top_down(Align::LEFT),
        |ui| {
            ScrollArea::vertical()
                .id_salt(("pad-editor-scroll", &sound.id))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    Frame::NONE
                        .inner_margin(Margin::symmetric(18, 20))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 22.0;

                            // Name field
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 7.0;
                                ui.label(
                                    egui::RichText::new("Name")
                                        .font(font_medium(13.0))
                                        .color(p.text),
                                );
                                let resp = text_input(
                                    ui,
                                    p,
                                    &mut state.editor_name_draft,
                                    "",
                                    ui.available_width(),
                                    60,
                                );
                                let enter =
                                    resp.has_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                                if enter || resp.lost_focus() {
                                    let trimmed = state.editor_name_draft.trim().to_owned();
                                    if trimmed.is_empty() {
                                        state.editor_name_draft = sound.name.clone();
                                    } else if trimmed != sound.name {
                                        let res = actions.controller.lock().update_sound(
                                            &sound.id,
                                            SoundPatch {
                                                name: Some(trimmed),
                                                ..Default::default()
                                            },
                                        );
                                        actions.run_or_toast(state, res);
                                    }
                                    if enter {
                                        resp.surrender_focus();
                                    }
                                }
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} long. kavoxctl can play it by this name.",
                                        format_duration(sound.duration_ms)
                                    ))
                                    .font(font_body(13.0))
                                    .color(p.muted),
                                );
                            });

                            // Color swatches
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 7.0;
                                ui.label(
                                    egui::RichText::new("Color")
                                        .font(font_medium(13.0))
                                        .color(p.text),
                                );
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::splat(10.0);
                                    for &hex in PALETTE {
                                        let col = parse_hex(hex);
                                        let selected = sound.color.eq_ignore_ascii_case(hex);
                                        let (rect, resp) = ui.allocate_exact_size(
                                            Vec2::splat(26.0),
                                            Sense::click(),
                                        );
                                        if resp.hovered() {
                                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                        }
                                        if ui.is_rect_visible(rect) {
                                            let painter = ui.painter();
                                            if selected {
                                                painter.circle_stroke(
                                                    rect.center(),
                                                    15.0,
                                                    Stroke::new(1.5, p.text),
                                                );
                                            }
                                            painter.circle_filled(rect.center(), 12.0, col);
                                        }
                                        if resp.clicked() && !selected {
                                            let res = actions.controller.lock().update_sound(
                                                &sound.id,
                                                SoundPatch {
                                                    color: Some(hex.to_owned()),
                                                    ..Default::default()
                                                },
                                            );
                                            actions.run_or_toast(state, res);
                                        }
                                    }
                                });
                            });

                            // Volume fader + preview play button
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 7.0;
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("Volume")
                                            .font(font_medium(13.0))
                                            .color(p.text),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{}%",
                                                (sound.volume * 100.0).round() as i32
                                            ))
                                            .font(font_body(13.0))
                                            .color(p.muted),
                                        );
                                    });
                                });

                                let fader_w = (ui.available_width() - 42.0).max(80.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(fader_w, 22.0),
                                        Layout::left_to_right(Align::Center),
                                        |ui| {
                                            if let Some(next_vol) = fader(
                                                ui,
                                                p,
                                                ui.make_persistent_id(("sound-vol", &sound.id)),
                                                sound.volume,
                                                2.0,
                                                None,
                                                false,
                                                p.panel,
                                            ) {
                                                if let Some(s) =
                                                    state.library.sounds.get_mut(&sound.id)
                                                {
                                                    s.volume = next_vol;
                                                }
                                                let res = actions.controller.lock().update_sound(
                                                    &sound.id,
                                                    SoundPatch {
                                                        volume: Some(next_vol),
                                                        ..Default::default()
                                                    },
                                                );
                                                actions.run_or_toast(state, res);
                                            }
                                        },
                                    );
                                    if icon_button(
                                        ui,
                                        p,
                                        IconName::Play,
                                        18.0,
                                        34.0,
                                        &format!("Preview {}", sound.name),
                                    )
                                    .clicked()
                                    {
                                        let res = actions.controller.lock().play_sound(&sound.id);
                                        actions.run_or_toast(state, res);
                                    }
                                });

                                if overridden {
                                    ui.label(
                                        egui::RichText::new("Not used right now: “Use one volume for every sound” is on in Settings → Audio, so the “All sounds” slider sets the volume instead. This value is kept for when you turn it off.")
                                            .font(font_body(13.0))
                                            .color(p.muted),
                                    );
                                }
                            });

                            // Hotkey recorder
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 7.0;
                                let rec_id = format!("sound-hk-{}", sound.id);
                                if let Some(next_hk) = hotkey_recorder(
                                    ui,
                                    p,
                                    state,
                                    &rec_id,
                                    "Hotkey",
                                    sound.hotkey.as_deref(),
                                ) {
                                    match actions.set_sound_hotkey(&sound.id, next_hk) {
                                        Ok(()) => {
                                            state.hotkey_errors.remove(&rec_id);
                                        }
                                        Err(err) => {
                                            state.hotkey_errors.insert(rec_id, err);
                                        }
                                    }
                                }
                                if !state.hotkeys_supported {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        ui.label(
                                            egui::RichText::new(
                                                "Global hotkeys don't work on Wayland. Bind",
                                            )
                                            .font(font_body(13.0))
                                            .color(p.muted),
                                        );
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "kavoxctl play \"{}\"",
                                                sound.name
                                            ))
                                            .font(font_mono(12.0))
                                            .color(p.text)
                                            .background_color(p.surface),
                                        );
                                        ui.label(
                                            egui::RichText::new("to a key in your compositor instead; Settings → Hotkeys shows how.")
                                                .font(font_body(13.0))
                                                .color(p.muted),
                                        );
                                    });
                                }
                            });

                            // When pressed again (Retrigger)
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 7.0;
                                ui.label(
                                    egui::RichText::new("When pressed again")
                                        .font(font_medium(13.0))
                                        .color(p.text),
                                );
                                if let Some(next_mode) = segmented(
                                    ui,
                                    p,
                                    sound.retrigger,
                                    &[
                                        (Retrigger::Restart, "Restart", Some(IconName::Restart)),
                                        (Retrigger::Overlap, "Overlap", Some(IconName::Overlap)),
                                        (Retrigger::Toggle, "Stop", Some(IconName::Toggle)),
                                    ],
                                ) {
                                    let res = actions.controller.lock().update_sound(
                                        &sound.id,
                                        SoundPatch {
                                            retrigger: Some(next_mode),
                                            ..Default::default()
                                        },
                                    );
                                    actions.run_or_toast(state, res);
                                }
                                ui.label(
                                    egui::RichText::new(mode_hint(sound.retrigger))
                                        .font(font_body(13.0))
                                        .color(p.muted),
                                );
                            });

                            // Loop switch
                            if let Some(looping) = switch_row(
                                ui,
                                p,
                                &format!("sound-loop-{}", sound.id),
                                "Loop",
                                Some("Repeats until you stop it."),
                                sound.looping,
                            ) {
                                let res = actions.controller.lock().update_sound(
                                    &sound.id,
                                    SoundPatch {
                                        looping: Some(looping),
                                        ..Default::default()
                                    },
                                );
                                actions.run_or_toast(state, res);
                            }
                        });
                });
        },
    );

    // Footer divider + Delete sound button
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
        .fill(p.panel)
        .inner_margin(Margin::symmetric(18, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if pill_button(
                ui,
                p,
                "Delete sound",
                Some(IconName::Trash),
                ButtonVariant::Danger,
                true,
            )
            .clicked()
            {
                state.confirm_delete_sound(&sound.id);
            }
        });
}
