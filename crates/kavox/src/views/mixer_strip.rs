//! Bottom mixer strip matching `src/components/MixerStrip.tsx` and `src/styles.css`.

use eframe::egui::{
    self, Align, Color32, CornerRadius, CursorIcon, Frame, Layout, Margin, Pos2, Rect, Sense,
    Stroke, StrokeKind, Ui, Vec2,
};
use kavox_core::VirtualStatus;

use crate::icons::{IconName, paint_icon};
use crate::state::{SettingsTab, UiState, mic_label};
use crate::theme::{Palette, font_body, with_alpha};
use crate::views::ActionCtx;
use crate::widgets::{ButtonVariant, fader, pill_button_with_keycaps};

pub fn show(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    // Top border line
    let top_rect = ui.max_rect();
    ui.painter().line_segment(
        [
            Pos2::new(top_rect.min.x, top_rect.min.y),
            Pos2::new(top_rect.max.x, top_rect.min.y),
        ],
        Stroke::new(1.0, p.line),
    );

    Frame::NONE
        .fill(p.panel)
        .inner_margin(Margin::symmetric(18, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let avail_w = ui.available_width();
            let wrap_end = avail_w < 840.0;

            if wrap_end {
                draw_channels(ui, p, state, actions, avail_w);
                ui.add_space(12.0);
                draw_end_controls(ui, p, state, actions);
            } else {
                let end_reserve = 296.0f32.min(avail_w * 0.34);
                let channels_w = (avail_w - end_reserve - 20.0).max(320.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 20.0;
                    ui.allocate_ui_with_layout(
                        Vec2::new(channels_w, 40.0),
                        Layout::left_to_right(Align::Center),
                        |ui| draw_channels(ui, p, state, actions, channels_w),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        draw_end_controls_rtl(ui, p, state, actions);
                    });
                });
            }
        });
}

fn draw_channels(
    ui: &mut Ui,
    p: &Palette,
    state: &mut UiState,
    actions: &mut ActionCtx<'_>,
    avail_w: f32,
) {
    let audio = state.settings.audio.clone();
    let levels = state.levels;
    let mic_off = audio.mic_muted || !audio.mic_passthrough;
    let count: usize = if audio.global_volume_override { 4 } else { 3 };
    let gap = 16.0;
    let min_col = 150.0;
    let cols = (((avail_w + gap) / (min_col + gap)).floor() as usize).clamp(1, count);
    let col_w = ((avail_w - gap * (cols as f32 - 1.0)) / cols as f32).max(135.0);

    let mut ch_idx = 0usize;
    let rows = count.div_ceil(cols);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 12.0;
        for _row in 0..rows {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for _col in 0..cols {
                    if ch_idx >= count {
                        break;
                    }
                    let slot = ch_idx;
                    ch_idx += 1;

                    let mapped = if audio.global_volume_override {
                        slot
                    } else if slot == 0 {
                        0
                    } else {
                        slot + 1
                    };

                    ui.allocate_ui_with_layout(
                        Vec2::new(col_w, 40.0),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            match mapped {
                                0 => {
                                    // Mic mute button + fader
                                    let muted = audio.mic_muted;
                                    let resp = draw_channel_icon(
                                        ui,
                                        p,
                                        if muted {
                                            IconName::MicOff
                                        } else {
                                            IconName::Mic
                                        },
                                        true,
                                        muted,
                                    )
                                    .on_hover_text(if muted {
                                        "Mic muted: sounds still play"
                                    } else {
                                        "Mute mic"
                                    });
                                    if resp.clicked() {
                                        let next = actions.controller.lock().set_mic_muted(None);
                                        state.settings.audio.mic_muted = next;
                                    }
                                    let label = if !audio.mic_passthrough {
                                        "Mic (not sent)"
                                    } else if muted {
                                        "Mic (muted)"
                                    } else {
                                        "Your mic"
                                    };
                                    if let Some(v) = draw_channel_body(
                                        ui,
                                        p,
                                        "ch-mic",
                                        label,
                                        audio.mic_gain,
                                        Some(levels.mic),
                                        mic_off,
                                    ) {
                                        state.update_audio_gain(actions.controller, |a| {
                                            a.mic_gain = v;
                                        });
                                    }
                                }
                                1 => {
                                    // Global volume override ("All sounds")
                                    draw_channel_icon(ui, p, IconName::Volume, false, false);
                                    if let Some(v) = draw_channel_body(
                                        ui,
                                        p,
                                        "ch-all-sounds",
                                        "All sounds",
                                        audio.global_sound_volume,
                                        None,
                                        false,
                                    ) {
                                        state.update_audio_gain(actions.controller, |a| {
                                            a.global_sound_volume = v;
                                        });
                                    }
                                }
                                2 => {
                                    // Sounds to mic
                                    draw_channel_icon(ui, p, IconName::Broadcast, false, false);
                                    if let Some(v) = draw_channel_body(
                                        ui,
                                        p,
                                        "ch-virtual",
                                        "Sounds to mic",
                                        audio.sounds_to_mic_gain,
                                        Some(levels.virtual_out),
                                        false,
                                    ) {
                                        state.update_audio_gain(actions.controller, |a| {
                                            a.sounds_to_mic_gain = v;
                                        });
                                    }
                                }
                                _ => {
                                    // Sounds in your ears (monitor)
                                    draw_channel_icon(ui, p, IconName::Headphones, false, false);
                                    let label = if audio.monitor_enabled {
                                        "Sounds in your ears"
                                    } else {
                                        "Your ears (off)"
                                    };
                                    if let Some(v) = draw_channel_body(
                                        ui,
                                        p,
                                        "ch-monitor",
                                        label,
                                        audio.sounds_to_monitor_gain,
                                        Some(levels.monitor),
                                        !audio.monitor_enabled,
                                    ) {
                                        state.update_audio_gain(actions.controller, |a| {
                                            a.sounds_to_monitor_gain = v;
                                        });
                                    }
                                }
                            }
                        },
                    );
                }
            });
        }
    });
}

fn draw_channel_icon(
    ui: &mut Ui,
    p: &Palette,
    icon: IconName,
    clickable: bool,
    is_muted: bool,
) -> egui::Response {
    let sense = if clickable {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(36.0), sense);
    if clickable && resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let border = if is_muted {
            with_alpha(p.red, 140)
        } else if clickable && resp.hovered() {
            p.line_strong
        } else {
            p.line
        };
        let fg = if is_muted {
            p.red_text
        } else if clickable && resp.hovered() {
            p.text
        } else {
            p.muted
        };
        painter.circle_stroke(rect.center(), 17.5, Stroke::new(1.0, border));
        paint_icon(painter, rect.center(), 18.0, icon, fg);
    }
    resp
}

fn draw_channel_body(
    ui: &mut Ui,
    p: &Palette,
    id_salt: &str,
    label: &str,
    value: f32,
    level: Option<f32>,
    dimmed: bool,
) -> Option<f32> {
    let mut next = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let row_w = ui.available_width().max(40.0);
        let (row_rect, _) = ui.allocate_exact_size(Vec2::new(row_w, 16.0), Sense::hover());
        if ui.is_rect_visible(row_rect) {
            let painter = ui.painter();
            let pct_text = format!("{}%", (value * 100.0).round() as i32);
            let pct_g = painter.layout_no_wrap(pct_text, font_body(12.0), p.text);
            let pct_x = row_rect.max.x - pct_g.size().x;
            let pct_y = row_rect.center().y - pct_g.size().y * 0.5;
            painter.galley(Pos2::new(pct_x, pct_y), pct_g, p.text);

            let lbl_max_x = (pct_x - 6.0).max(row_rect.min.x);
            let lbl_clip = Rect::from_min_max(row_rect.min, Pos2::new(lbl_max_x, row_rect.max.y));
            let lbl_g = painter.layout_no_wrap(label.to_owned(), font_body(12.0), p.muted);
            let lbl_y = row_rect.center().y - lbl_g.size().y * 0.5;
            painter.with_clip_rect(lbl_clip).galley(
                Pos2::new(row_rect.min.x, lbl_y),
                lbl_g,
                p.muted,
            );
        }
        next = fader(
            ui,
            p,
            ui.make_persistent_id(id_salt),
            value,
            2.0,
            level,
            dimmed,
            p.panel,
        );
    });
    next
}

fn draw_end_controls(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(10.0);
        draw_stop_all(ui, p, state, actions);
        draw_output_status(ui, p, state);
    });
}

fn draw_end_controls_rtl(
    ui: &mut Ui,
    p: &Palette,
    state: &mut UiState,
    actions: &mut ActionCtx<'_>,
) {
    ui.spacing_mut().item_spacing.x = 10.0;
    draw_output_status(ui, p, state);
    draw_stop_all(ui, p, state, actions);
}

fn draw_stop_all(ui: &mut Ui, p: &Palette, state: &UiState, actions: &mut ActionCtx<'_>) {
    let hk = state.settings.hotkeys.stop_all.as_deref();
    let resp = pill_button_with_keycaps(
        ui,
        p,
        "Stop all",
        Some(IconName::Stop),
        hk,
        Some(state.platform),
        ButtonVariant::Default,
        true,
    );
    if resp.hovered() {
        // Subtle red hover border matching `.stop-all:hover`
        ui.painter().rect_stroke(
            resp.rect,
            CornerRadius::same(18),
            Stroke::new(1.0, with_alpha(p.red, 140)),
            StrokeKind::Inside,
        );
    }
    if resp.clicked() {
        actions.controller.lock().stop_all();
    }
}

fn draw_output_status(ui: &mut Ui, p: &Palette, state: &mut UiState) {
    let label = mic_label(&state.virtual_status).map(str::to_owned);
    let fallback = state.streams.mic.as_ref().is_some_and(|s| s.fallback)
        || state.streams.monitor.as_ref().is_some_and(|s| s.fallback);

    let tone = if label.is_none() || state.streams.virtual_out.is_none() {
        Tone::Bad
    } else if matches!(
        state.virtual_status,
        VirtualStatus::CableIsDefaultOutput { .. }
    ) || fallback
    {
        Tone::Warn
    } else {
        Tone::Ok
    };

    let text = if tone == Tone::Bad {
        "Voice apps can’t hear Kavox".to_owned()
    } else {
        let short = label
            .as_deref()
            .unwrap_or("")
            .split('(')
            .next()
            .unwrap_or("")
            .trim();
        format!("Voice apps use {short}")
    };

    let tooltip = match &label {
        Some(l) => format!("Select \"{l}\" as the microphone in Discord, OBS or your game"),
        None => "Open audio settings".into(),
    };

    let font = font_body(13.0);
    let g = ui
        .painter()
        .layout_no_wrap(text.clone(), font.clone(), p.muted);
    let w = (13.0 + 8.0 + 9.0 + g.size().x + 14.0).min(280.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 36.0), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let border = if resp.hovered() {
            p.line_strong
        } else {
            p.line
        };
        let fg = if resp.hovered() { p.text } else { p.muted };
        painter.rect_stroke(
            rect,
            CornerRadius::same(18),
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );

        let dot_c = Pos2::new(rect.min.x + 17.0, rect.center().y);
        match tone {
            Tone::Ok => painter.circle_filled(dot_c, 4.0, p.red),
            Tone::Warn => painter.circle_filled(dot_c, 4.0, p.amber),
            Tone::Bad => painter.circle_stroke(dot_c, 3.25, Stroke::new(1.5, p.muted)),
        };

        let clip = Rect::from_min_max(
            Pos2::new(rect.min.x + 30.0, rect.min.y),
            Pos2::new(rect.max.x - 12.0, rect.max.y),
        );
        let tg = painter.layout_no_wrap(text, font, fg);
        painter.with_clip_rect(clip).galley(
            Pos2::new(clip.min.x, rect.center().y - tg.size().y * 0.5),
            tg,
            fg,
        );
    }
    let resp = resp.on_hover_text(tooltip);
    if resp.clicked() {
        state.open_settings(SettingsTab::Audio);
    }
}

#[derive(PartialEq, Eq)]
enum Tone {
    Ok,
    Warn,
    Bad,
}

#[allow(dead_code)]
fn _keep_color32(_: Color32) {}
