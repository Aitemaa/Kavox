//! Voice changer view matching `src/components/VoiceBoard.tsx`.

use eframe::egui::{
    self, Align, Color32, CornerRadius, CursorIcon, Layout, Pos2, ScrollArea, Sense, Ui, Vec2,
};

use crate::hotkey;
use crate::icons::{IconName, paint_icon};
use crate::state::UiState;
use crate::theme::{
    Palette, RADIUS_PAD, font_body, font_heading, font_medium, parse_hex, with_alpha,
};
use crate::views::ActionCtx;
use crate::voices::{CUSTOM, NORMAL, VOICES, VoiceInfo, format_semitones, voice_name};
use crate::widgets::{hotkey_recorder, linear_slider, paint_keycaps_at, switch_row, switch_toggle};

pub fn show(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    let narrow = ui.ctx().content_rect().width() < 720.0;
    let pad_x = if narrow { 16.0 } else { 20.0 };
    let pad_top = if narrow { 14.0 } else { 20.0 };

    ScrollArea::vertical()
        .id_salt("voice-board-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: pad_x as i8,
                    right: pad_x as i8,
                    top: pad_top as i8,
                    bottom: 28,
                })
                .show(ui, |ui| {
                    let max_w = ui.available_width().min(920.0);
                    ui.set_width(max_w);
                    ui.spacing_mut().item_spacing.y = 22.0;

                    let voice = state.settings.voice.clone();
                    let live = voice.enabled && voice.preset != NORMAL;
                    let dial = if voice.preset == CUSTOM {
                        voice.semitones
                    } else {
                        VOICES
                            .iter()
                            .find(|v| v.id == voice.preset)
                            .map(|v| v.semitones)
                            .unwrap_or(0.0)
                    };

                    // Header row: title + description + master toggle switch
                    ui.horizontal_top(|ui| {
                        let text_w = (max_w - 64.0).max(160.0);
                        ui.allocate_ui_with_layout(
                            Vec2::new(text_w, 0.0),
                            Layout::top_down(Align::LEFT),
                            |ui| {
                                ui.spacing_mut().item_spacing.y = 4.0;
                                ui.label(
                                    egui::RichText::new("Voice changer")
                                        .font(font_heading(20.0))
                                        .color(p.text),
                                );
                                let subtitle = if live {
                                    format!(
                                        "You sound like {}. Everyone on the call hears it.",
                                        voice_name(&voice.preset)
                                    )
                                } else {
                                    "Pick a voice. Only your mic changes: the sounds on your pads stay as they are.".into()
                                };
                                ui.label(
                                    egui::RichText::new(subtitle)
                                        .font(font_body(14.0))
                                        .color(p.muted),
                                );
                            },
                        );
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            if switch_toggle(
                                ui,
                                p,
                                ui.make_persistent_id("voice-master-switch"),
                                voice.enabled,
                            )
                            .clicked()
                            {
                                state.update_voice(actions.controller, |v| {
                                    v.enabled = !v.enabled;
                                });
                            }
                        });
                    });

                    // Voice tiles grid (Normal + 8 presets)
                    let gap = 12.0;
                    let min_tile_w = if narrow { 132.0 } else { 156.0 };
                    let cols = (((max_w + gap) / (min_tile_w + gap)).floor() as usize).max(1);
                    let tile_w = (max_w - gap * (cols as f32 - 1.0)) / cols as f32;
                    let tile_h = 132.0;

                    let total = 1 + VOICES.len();
                    let rows = total.div_ceil(cols);
                    let normal_selected = voice.preset == NORMAL || !voice.enabled;

                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = gap;
                        for row in 0..rows {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                for col in 0..cols {
                                    let idx = row * cols + col;
                                    if idx == 0 {
                                        if draw_voice_tile(
                                            ui,
                                            p,
                                            state.platform,
                                            "Normal",
                                            IconName::Mic,
                                            p.faint,
                                            None,
                                            "As you are",
                                            normal_selected,
                                            Vec2::new(tile_w, tile_h),
                                        )
                                        .clicked()
                                        {
                                            state.update_voice(actions.controller, |v| {
                                                v.preset = NORMAL.into();
                                                v.enabled = false;
                                            });
                                        }
                                    } else if idx - 1 < VOICES.len() {
                                        let info: &VoiceInfo = &VOICES[idx - 1];
                                        let selected = voice.enabled && voice.preset == info.id;
                                        let hk = voice.hotkeys.get(info.id).map(String::as_str);
                                        let info_text = if info.wobble {
                                            "Wobbles".into()
                                        } else {
                                            format_semitones(info.semitones)
                                        };
                                        if draw_voice_tile(
                                            ui,
                                            p,
                                            state.platform,
                                            info.name,
                                            info.icon,
                                            parse_hex(info.color),
                                            hk,
                                            &info_text,
                                            selected,
                                            Vec2::new(tile_w, tile_h),
                                        )
                                        .clicked()
                                        {
                                            state.update_voice(actions.controller, |v| {
                                                v.preset = info.id.into();
                                                v.enabled = !selected;
                                            });
                                        }
                                    }
                                }
                            });
                        }
                    });

                    // Controls: Pitch slider + Hear myself switch (2 columns when wide, 1 column when narrow)
                    if narrow {
                        draw_pitch_control(ui, p, state, actions, dial);
                        draw_hear_myself(ui, p, state, actions, voice.hear_myself);
                    } else {
                        let col_w = (max_w - 32.0) * 0.5;
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 32.0;
                            ui.allocate_ui_with_layout(
                                Vec2::new(col_w, 0.0),
                                Layout::top_down(Align::LEFT),
                                |ui| draw_pitch_control(ui, p, state, actions, dial),
                            );
                            ui.allocate_ui_with_layout(
                                Vec2::new(col_w, 0.0),
                                Layout::top_down(Align::LEFT),
                                |ui| draw_hear_myself(ui, p, state, actions, voice.hear_myself),
                            );
                        });
                    }

                    // Collapsible Hotkeys section
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 10.0;
                        let arrow = if state.voice_hotkeys_open { "▾" } else { "▸" };
                        let summary_resp = ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("{arrow} Hotkeys"))
                                    .font(font_medium(13.0))
                                    .color(p.muted),
                            )
                            .sense(Sense::click()),
                        );
                        if summary_resp.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        if summary_resp.clicked() {
                            state.voice_hotkeys_open = !state.voice_hotkeys_open;
                        }

                        if state.voice_hotkeys_open {
                            let mut hint = "Press a voice's key to switch to it, and press it again to go back to your normal voice.".to_owned();
                            if !state.hotkeys_supported {
                                hint.push_str(" Global hotkeys aren’t available on Wayland, so these only save for later.");
                            }
                            ui.label(egui::RichText::new(hint).font(font_body(13.0)).color(p.muted));

                            let hk_cols = (((max_w + 24.0) / 244.0).floor() as usize).max(1);
                            let hk_w = (max_w - 24.0 * (hk_cols as f32 - 1.0)) / hk_cols as f32;
                            let hk_rows = VOICES.len().div_ceil(hk_cols);
                            for row in 0..hk_rows {
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = 24.0;
                                    for col in 0..hk_cols {
                                        let i = row * hk_cols + col;
                                        if i < VOICES.len() {
                                            let v = &VOICES[i];
                                            let rec_id = format!("voice-hk-{}", v.id);
                                            let cur = state
                                                .settings
                                                .voice
                                                .hotkeys
                                                .get(v.id)
                                                .cloned();
                                            ui.allocate_ui_with_layout(
                                                Vec2::new(hk_w, 0.0),
                                                Layout::top_down(Align::LEFT),
                                                |ui| {
                                                    if let Some(next_hk) = hotkey_recorder(
                                                        ui,
                                                        p,
                                                        state,
                                                        &rec_id,
                                                        v.name,
                                                        cur.as_deref(),
                                                    ) {
                                                        match actions
                                                            .set_voice_hotkey(v.id, next_hk)
                                                        {
                                                            Ok(()) => {
                                                                state.hotkey_errors.remove(&rec_id);
                                                            }
                                                            Err(err) => {
                                                                state
                                                                    .hotkey_errors
                                                                    .insert(rec_id, err);
                                                            }
                                                        }
                                                    }
                                                },
                                            );
                                        }
                                    }
                                });
                            }
                        }
                    });
                });
        });
}

fn draw_pitch_control(
    ui: &mut Ui,
    p: &Palette,
    state: &mut UiState,
    actions: &mut ActionCtx<'_>,
    dial: f32,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 7.0;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Pitch")
                    .font(font_medium(13.0))
                    .color(p.text),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format_semitones(dial))
                        .font(font_body(13.0))
                        .color(p.muted),
                );
            });
        });
        if let Some(semitones) = linear_slider(ui, p, dial, -12.0, 12.0, 0.5, Some(0.0), p.bg) {
            state.update_voice_pitch(actions.controller, |v| {
                v.preset = CUSTOM.into();
                v.semitones = semitones;
                v.enabled = true;
            });
        }
        ui.label(
            egui::RichText::new("Drag to make your own voice. Low is deeper, high is squeakier.")
                .font(font_body(13.0))
                .color(p.muted),
        );
    });
}

fn draw_hear_myself(
    ui: &mut Ui,
    p: &Palette,
    state: &mut UiState,
    actions: &mut ActionCtx<'_>,
    checked: bool,
) {
    if let Some(next) = switch_row(
        ui,
        p,
        "voice-hear-myself",
        "Hear myself",
        Some(
            "Plays your changed voice in your headphones. Use headphones, or the mic will pick it up.",
        ),
        checked,
    ) {
        state.update_voice(actions.controller, |v| v.hear_myself = next);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_voice_tile(
    ui: &mut Ui,
    p: &Palette,
    platform: &str,
    name: &str,
    icon: IconName,
    dot_color: Color32,
    hotkey_str: Option<&str>,
    info_label: &str,
    selected: bool,
    size: Vec2,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let draw_rect = if response.is_pointer_button_down_on() {
            rect.translate(Vec2::new(0.0, 1.0))
        } else {
            rect
        };

        let bg = if selected {
            p.lit
        } else if response.hovered() {
            p.surface_2
        } else {
            p.surface
        };
        let fg = if selected { p.on_lit } else { p.text };
        let sub_fg = if selected { p.on_lit } else { p.muted };

        painter.rect_filled(draw_rect, CornerRadius::same(RADIUS_PAD), bg);

        // Top-left dot (static subtle halo when selected; avoids continuous idle repaints)
        let dot_center = Pos2::new(draw_rect.min.x + 19.0, draw_rect.min.y + 19.0);
        if selected {
            painter.circle_filled(dot_center, 7.5, with_alpha(p.on_lit, 46));
            painter.circle_filled(dot_center, 4.0, p.on_lit);
        } else {
            painter.circle_filled(dot_center, 4.0, dot_color.gamma_multiply(0.55));
        }

        // Top-right 26px voice icon
        let icon_sz = if response.hovered() && !selected {
            28.0
        } else {
            26.0
        };
        paint_icon(
            painter,
            Pos2::new(draw_rect.max.x - 25.0, draw_rect.min.y + 25.0),
            icon_sz,
            icon,
            sub_fg,
        );

        // Name
        let ng = painter.layout_no_wrap(name.to_owned(), font_medium(15.0), fg);
        painter.galley(
            Pos2::new(draw_rect.min.x + 14.0, draw_rect.min.y + 42.0),
            ng,
            fg,
        );

        // Footer: hotkey (bottom-left) + semitones/Wobbles (bottom-right)
        let foot_y = draw_rect.max.y - 21.0;
        if let Some(hk) = hotkey_str {
            let caps = hotkey::keycaps(hk, platform);
            paint_keycaps_at(
                painter,
                ui,
                p,
                Pos2::new(draw_rect.min.x + 14.0, foot_y),
                &caps,
                true,
            );
        }
        let ig = painter.layout_no_wrap(info_label.to_owned(), font_body(12.0), sub_fg);
        painter.galley(
            Pos2::new(
                draw_rect.max.x - 14.0 - ig.size().x,
                foot_y - ig.size().y * 0.5,
            ),
            ig,
            sub_fg,
        );
    }

    response
}
