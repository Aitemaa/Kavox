//! Reusable Kavox widgets faithfully matching `src/components/ui.tsx` and `src/styles.css`.

use eframe::egui::{
    self, Align, Color32, CornerRadius, CursorIcon, Frame, Id, Layout, Margin, Order, Pos2, Rect,
    Response, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
use kavox_core::DeviceInfo;

use crate::hotkey::{self, RecordResult};
use crate::icons::{IconName, paint_icon};
use crate::state::{UiState, meter_fill};
use crate::theme::{Palette, RADIUS_FIELD, RADIUS_PANEL, font_body, font_medium, mix, with_alpha};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Default,
    Primary,
    Danger,
    Quiet,
}

pub fn pill_button(
    ui: &mut Ui,
    p: &Palette,
    label: &str,
    icon: Option<IconName>,
    variant: ButtonVariant,
    enabled: bool,
) -> Response {
    pill_button_with_keycaps(ui, p, label, icon, None, None, variant, enabled)
}

#[allow(clippy::too_many_arguments)]
pub fn pill_button_with_keycaps(
    ui: &mut Ui,
    p: &Palette,
    label: &str,
    icon: Option<IconName>,
    hotkey_str: Option<&str>,
    platform: Option<&str>,
    variant: ButtonVariant,
    enabled: bool,
) -> Response {
    let font = font_medium(14.0);
    let text_galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), p.text);
    let icon_sz = if matches!(icon, Some(IconName::Stop)) {
        14.0
    } else {
        17.0
    };
    let mut inner_w = text_galley.size().x;
    if icon.is_some() {
        inner_w += icon_sz + 8.0;
    }
    let caps: Vec<String> = match (hotkey_str, platform) {
        (Some(hk), Some(plat)) => hotkey::keycaps(hk, plat),
        _ => Vec::new(),
    };
    let caps_w = if caps.is_empty() {
        0.0
    } else {
        8.0 + measure_keycaps(ui, &caps, true)
    };
    inner_w += caps_w;

    let pad_x = 16.0;
    let height = 36.0;
    let size = Vec2::new(inner_w + pad_x * 2.0, height);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);

    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    if ui.is_rect_visible(rect) {
        let hovered = enabled && response.hovered();
        let (bg, border, fg) = match variant {
            ButtonVariant::Default => (
                if hovered {
                    p.surface
                } else {
                    Color32::TRANSPARENT
                },
                p.line_strong,
                p.text,
            ),
            ButtonVariant::Primary => (
                if hovered {
                    mix(p.lit, p.muted, 0.86)
                } else {
                    p.lit
                },
                Color32::TRANSPARENT,
                p.on_lit,
            ),
            ButtonVariant::Danger => (
                if hovered {
                    with_alpha(p.red, 30)
                } else {
                    Color32::TRANSPARENT
                },
                with_alpha(p.red, 128),
                p.red_text,
            ),
            ButtonVariant::Quiet => (
                Color32::TRANSPARENT,
                Color32::TRANSPARENT,
                if hovered { p.text } else { p.muted },
            ),
        };

        let alpha_mul = if enabled { 1.0 } else { 0.45 };
        let bg = bg.gamma_multiply(alpha_mul);
        let border = border.gamma_multiply(alpha_mul);
        let fg = fg.gamma_multiply(alpha_mul);

        let painter = ui.painter();
        let radius = CornerRadius::same(18);
        if bg != Color32::TRANSPARENT {
            painter.rect_filled(rect, radius, bg);
        }
        if border != Color32::TRANSPARENT {
            painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
        }

        let mut x = rect.min.x + pad_x;
        let cy = rect.center().y;
        if let Some(ic) = icon {
            paint_icon(painter, Pos2::new(x + icon_sz * 0.5, cy), icon_sz, ic, fg);
            x += icon_sz + 8.0;
        }
        let galley = painter.layout_no_wrap(label.to_owned(), font, fg);
        let text_pos = Pos2::new(x, cy - galley.size().y * 0.5);
        x += galley.size().x;
        painter.galley(text_pos, galley, fg);

        if !caps.is_empty() {
            x += 8.0;
            paint_keycaps_at(painter, ui, p, Pos2::new(x, cy), &caps, true);
        }
    }

    response
}

/// Circular icon-only button matching `.icon-button` (`34x34` default).
pub fn icon_button(
    ui: &mut Ui,
    p: &Palette,
    icon: IconName,
    icon_size: f32,
    btn_size: f32,
    tooltip: &str,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(btn_size), Sense::click());
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() {
            painter.circle_filled(rect.center(), btn_size * 0.5, p.surface);
        }
        let color = if response.hovered() { p.text } else { p.muted };
        paint_icon(painter, rect.center(), icon_size, icon, color);
    }
    if !tooltip.is_empty() {
        response.on_hover_text(tooltip)
    } else {
        response
    }
}

/// Underlined inline link button matching `.link`.
pub fn link_button(ui: &mut Ui, p: &Palette, label: &str) -> Response {
    let font = font_medium(14.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), p.text);
    let size = Vec2::new(galley.size().x, galley.size().y + 4.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.galley(rect.min, galley, p.text);
        let y = rect.max.y - 1.0;
        painter.line_segment(
            [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
            Stroke::new(1.5, p.red),
        );
    }
    response
}

/// Pill segmented control matching `.segmented`.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    p: &Palette,
    current: T,
    options: &[(T, &str, Option<IconName>)],
) -> Option<T> {
    let mut changed = None;
    Frame::NONE
        .fill(p.surface)
        .corner_radius(CornerRadius::same(18))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for &(val, label, icon) in options {
                    let active = val == current;
                    let font = font_medium(13.0);
                    let galley =
                        ui.painter()
                            .layout_no_wrap(label.to_owned(), font.clone(), p.text);
                    let icon_sz = 15.0;
                    let mut w = galley.size().x + 26.0;
                    if icon.is_some() {
                        w += icon_sz + 6.0;
                    }
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 30.0), Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if ui.is_rect_visible(rect) {
                        let painter = ui.painter();
                        if active {
                            painter.rect_filled(rect, CornerRadius::same(15), p.lit);
                        }
                        let fg = if active {
                            p.on_lit
                        } else if resp.hovered() {
                            p.text
                        } else {
                            p.muted
                        };
                        let mut x = rect.min.x + 13.0;
                        let cy = rect.center().y;
                        if let Some(ic) = icon {
                            paint_icon(painter, Pos2::new(x + icon_sz * 0.5, cy), icon_sz, ic, fg);
                            x += icon_sz + 6.0;
                        }
                        let g = painter.layout_no_wrap(label.to_owned(), font, fg);
                        painter.galley(Pos2::new(x, cy - g.size().y * 0.5), g, fg);
                    }
                    if resp.clicked() && !active {
                        changed = Some(val);
                    }
                }
            });
        });
    changed
}

/// Toggle switch widget (`40x24`) matching `.switch`.
pub fn switch_toggle(ui: &mut Ui, p: &Palette, id: Id, checked: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(40.0, 24.0), Sense::click());
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time(id, checked, 0.15);
        let painter = ui.painter();
        let bg = mix(p.lit, p.surface_2, t);
        let thumb_color = mix(p.on_lit, p.muted, t);
        painter.rect_filled(rect, CornerRadius::same(12), bg);
        let cx = rect.min.x + 12.0 + t * 16.0;
        painter.circle_filled(Pos2::new(cx, rect.center().y), 9.0, thumb_color);
    }
    response
}

/// Label + optional hint on the left, switch on the right matching `.switch-field`.
pub fn switch_row(
    ui: &mut Ui,
    p: &Palette,
    id_salt: &str,
    label: &str,
    description: Option<&str>,
    checked: bool,
) -> Option<bool> {
    let mut toggled = None;
    let total_w = ui.available_width();
    let text_w = (total_w - 56.0).max(80.0);

    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(text_w, 0.0),
            Layout::top_down(Align::LEFT),
            |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(
                    egui::RichText::new(label)
                        .font(font_medium(13.0))
                        .color(p.text),
                );
                if let Some(desc) = description {
                    ui.label(
                        egui::RichText::new(desc)
                            .font(font_body(13.0))
                            .color(p.muted),
                    );
                }
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let resp = switch_toggle(ui, p, ui.make_persistent_id(id_salt), checked);
            if resp.clicked() {
                toggled = Some(!checked);
            }
        });
    });
    toggled
}

/// Perceptual gain fader (`position^2 * max`) with optional live peak meter behind the thumb.
#[allow(clippy::too_many_arguments)]
pub fn fader(
    ui: &mut Ui,
    p: &Palette,
    id: Id,
    value: f32,
    max: f32,
    level: Option<f32>,
    level_dimmed: bool,
    thumb_ring_bg: Color32,
) -> Option<f32> {
    let width = ui.available_width().max(40.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::click_and_drag());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let thumb_r = 8.0;
    let track_left = rect.min.x + thumb_r;
    let track_right = (rect.max.x - thumb_r).max(track_left + 1.0);
    let track_w = track_right - track_left;

    let mut next_value = None;
    if (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let pos = ((pointer.x - track_left) / track_w).clamp(0.0, 1.0);
        let raw = max * pos * pos;
        let rounded = (raw * 1000.0).round() / 1000.0;
        if (rounded - value).abs() > 0.0005 {
            next_value = Some(rounded);
        }
    }

    let display_val = next_value.unwrap_or(value);
    let position = (display_val.max(0.0) / max).sqrt().clamp(0.0, 1.0);

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let cy = rect.center().y;
        let track_rect = Rect::from_min_max(
            Pos2::new(rect.min.x, cy - 1.5),
            Pos2::new(rect.max.x, cy + 1.5),
        );
        painter.rect_filled(track_rect, CornerRadius::same(2), p.line_strong);

        if let Some(peak) = level {
            let target = meter_fill(peak);
            let smooth = ui
                .ctx()
                .animate_value_with_time(id.with("lvl"), target, 0.07);
            if smooth > 0.002 {
                let fill_w = rect.width() * smooth;
                let fill_rect =
                    Rect::from_min_size(Pos2::new(rect.min.x, cy - 1.5), Vec2::new(fill_w, 3.0));
                let mut fill_col = if target > 0.95 { p.red } else { p.lit };
                if level_dimmed {
                    fill_col = fill_col.gamma_multiply(0.3);
                }
                painter.rect_filled(fill_rect, CornerRadius::same(2), fill_col);
            }
        }

        let thumb_cx = track_left + position * track_w;
        let thumb_center = Pos2::new(thumb_cx, cy);
        painter.circle_filled(thumb_center, thumb_r, thumb_ring_bg);
        painter.circle_filled(thumb_center, thumb_r - 3.0, p.lit);
    }

    next_value
}

/// Linear slider with optional center tick (used for Voice Pitch `-12..+12`).
#[allow(clippy::too_many_arguments)]
pub fn linear_slider(
    ui: &mut Ui,
    p: &Palette,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    center: Option<f32>,
    thumb_ring_bg: Color32,
) -> Option<f32> {
    let width = ui.available_width().max(40.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::click_and_drag());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let thumb_r = 8.0;
    let track_left = rect.min.x + thumb_r;
    let track_right = (rect.max.x - thumb_r).max(track_left + 1.0);
    let track_w = track_right - track_left;

    let mut next_value = None;
    if (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let t = ((pointer.x - track_left) / track_w).clamp(0.0, 1.0);
        let raw = min + t * (max - min);
        let stepped = if step > 0.0 {
            (raw / step).round() * step
        } else {
            raw
        }
        .clamp(min, max);
        if (stepped - value).abs() > 1e-4 {
            next_value = Some(stepped);
        }
    }

    let display = next_value.unwrap_or(value).clamp(min, max);
    let t = ((display - min) / (max - min)).clamp(0.0, 1.0);

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let cy = rect.center().y;
        let track_rect = Rect::from_min_max(
            Pos2::new(rect.min.x, cy - 1.5),
            Pos2::new(rect.max.x, cy + 1.5),
        );
        painter.rect_filled(track_rect, CornerRadius::same(2), p.line_strong);

        if let Some(c) = center {
            let ct = ((c - min) / (max - min)).clamp(0.0, 1.0);
            let tx = track_left + ct * track_w;
            let tick_rect = Rect::from_center_size(Pos2::new(tx, cy), Vec2::new(2.0, 11.0));
            painter.rect_filled(tick_rect, CornerRadius::same(1), p.muted);
        }

        let thumb_center = Pos2::new(track_left + t * track_w, cy);
        painter.circle_filled(thumb_center, thumb_r, thumb_ring_bg);
        painter.circle_filled(thumb_center, thumb_r - 3.0, p.lit);
    }

    next_value
}

/// Standalone horizontal level meter matching `.level-bar`.
pub fn level_bar(ui: &mut Ui, p: &Palette, id: Id, peak: f32) {
    let width = ui.available_width().max(40.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 4.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(2), p.surface_2);
        let target = meter_fill(peak);
        let smooth = ui.ctx().animate_value_with_time(id, target, 0.07);
        if smooth > 0.002 {
            let fill_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width() * smooth, 4.0));
            let col = if target > 0.95 { p.red } else { p.lit };
            painter.rect_filled(fill_rect, CornerRadius::same(2), col);
        }
    }
}

pub fn measure_keycaps(ui: &Ui, caps: &[String], small: bool) -> f32 {
    let font = font_medium(if small { 11.0 } else { 12.0 });
    let min_w = if small { 18.0 } else { 22.0 };
    let pad_x = if small { 5.0 } else { 6.0 };
    let mut total = 0.0;
    for (i, cap) in caps.iter().enumerate() {
        if i > 0 {
            total += 3.0;
        }
        let g = ui
            .painter()
            .layout_no_wrap(cap.clone(), font.clone(), Color32::WHITE);
        total += (g.size().x + pad_x * 2.0).max(min_w);
    }
    total
}

pub fn paint_keycaps_at(
    painter: &egui::Painter,
    ui: &Ui,
    p: &Palette,
    left_center: Pos2,
    caps: &[String],
    small: bool,
) {
    let font = font_medium(if small { 11.0 } else { 12.0 });
    let min_w = if small { 18.0 } else { 22.0 };
    let pad_x = if small { 5.0 } else { 6.0 };
    let h = if small { 17.0 } else { 21.0 };
    let fg = if small { p.muted } else { p.text };

    let mut x = left_center.x;
    for (i, cap) in caps.iter().enumerate() {
        if i > 0 {
            x += 3.0;
        }
        let g = ui.painter().layout_no_wrap(cap.clone(), font.clone(), fg);
        let w = (g.size().x + pad_x * 2.0).max(min_w);
        let rect = Rect::from_min_size(Pos2::new(x, left_center.y - h * 0.5), Vec2::new(w, h));
        painter.rect_stroke(
            rect,
            CornerRadius::same(6),
            Stroke::new(1.0, p.line_strong),
            StrokeKind::Inside,
        );
        painter.galley(
            Pos2::new(
                rect.center().x - g.size().x * 0.5,
                rect.center().y - g.size().y * 0.5,
            ),
            g,
            fg,
        );
        x += w;
    }
}

/// Interactive hotkey recorder matching `HotkeyRecorder` in `src/components/ui.tsx`.
/// Returns `Some(new_hotkey)` when the user commits or clears a hotkey.
pub fn hotkey_recorder(
    ui: &mut Ui,
    p: &Palette,
    state: &mut UiState,
    recorder_id: &str,
    label: &str,
    value: Option<&str>,
) -> Option<Option<String>> {
    let mut committed: Option<Option<String>> = None;
    let recording = state.recording_hotkey.as_deref() == Some(recorder_id);

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 7.0;
        ui.label(egui::RichText::new(label).font(font_medium(13.0)).color(p.text));

        let show_clear = value.is_some() && !recording;
        let total_w = ui.available_width();
        let btn_w = if show_clear {
            (total_w - 40.0).max(100.0)
        } else {
            total_w.max(100.0)
        };

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(btn_w, 36.0), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                state.hotkey_errors.remove(recorder_id);
                if recording {
                    state.recording_hotkey = None;
                } else {
                    state.recording_hotkey = Some(recorder_id.to_owned());
                }
            }

            if recording {
                // Capture next key press while recording.
                let mut key_press: Option<(egui::Key, egui::Modifiers)> = None;
                ui.input(|i| {
                    for ev in &i.events {
                        if let egui::Event::Key {
                            key,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } = ev
                        {
                            key_press = Some((*key, *modifiers));
                            break;
                        }
                    }
                });
                if let Some((key, mods)) = key_press {
                    match hotkey::from_egui_key(key, mods) {
                        RecordResult::Cancel => {
                            state.recording_hotkey = None;
                        }
                        RecordResult::Clear => {
                            state.recording_hotkey = None;
                            committed = Some(None);
                        }
                        RecordResult::Set(hk) => {
                            state.recording_hotkey = None;
                            committed = Some(Some(hk));
                        }
                        RecordResult::Unsupported(name) => {
                            state.hotkey_errors.insert(
                                recorder_id.to_owned(),
                                format!("{name} can't be used as a hotkey. Try a letter, number, F-key or numpad key."),
                            );
                        }
                    }
                } else if ui.input(|i| i.pointer.any_pressed()) && !resp.hovered() {
                    state.recording_hotkey = None;
                }
            }

            if ui.is_rect_visible(rect) {
                let painter = ui.painter();
                painter.rect_filled(rect, CornerRadius::same(RADIUS_FIELD), p.surface);
                let border = if recording {
                    with_alpha(p.red, 153)
                } else if resp.hovered() {
                    p.line_strong
                } else {
                    Color32::TRANSPARENT
                };
                if border != Color32::TRANSPARENT {
                    painter.rect_stroke(
                        rect,
                        CornerRadius::same(RADIUS_FIELD),
                        Stroke::new(1.0, border),
                        StrokeKind::Inside,
                    );
                }

                let mut x = rect.min.x + 12.0;
                let cy = rect.center().y;
                if recording {
                    painter.circle_filled(Pos2::new(x + 3.5, cy), 3.5, p.red);
                    x += 15.0;
                    let g = painter.layout_no_wrap(
                        "Press a key combination…".into(),
                        font_body(14.0),
                        p.text,
                    );
                    painter.galley(Pos2::new(x, cy - g.size().y * 0.5), g, p.text);
                } else if let Some(hk) = value {
                    let caps = hotkey::keycaps(hk, state.platform);
                    paint_keycaps_at(painter, ui, p, Pos2::new(x, cy), &caps, false);
                } else {
                    let g = painter.layout_no_wrap("Not set".into(), font_body(14.0), p.muted);
                    painter.galley(Pos2::new(x, cy - g.size().y * 0.5), g, p.muted);
                }
            }

            if show_clear
                && icon_button(ui, p, IconName::Close, 16.0, 34.0, &format!("Clear {label}"))
                    .clicked()
            {
                committed = Some(None);
            }
        });

        if recording {
            ui.label(
                egui::RichText::new("Esc cancels, Backspace clears.")
                    .font(font_body(13.0))
                    .color(p.muted),
            );
        } else if let Some(hk) = value
            && hotkey::is_risky(hk)
        {
            ui.label(
                egui::RichText::new("Other apps won't receive this key while Kavox runs. Add Ctrl or Alt, or use F13–F24 or the numpad.")
                    .font(font_body(13.0))
                    .color(p.amber),
            );
        }

        if let Some(err) = state.hotkey_errors.get(recorder_id) {
            ui.label(
                egui::RichText::new(err)
                    .font(font_body(13.0))
                    .color(p.red_text),
            );
        }
    });

    committed
}

/// Audio device select matching `DeviceSelect`.
pub fn device_select(
    ui: &mut Ui,
    p: &Palette,
    id_salt: &str,
    label: &str,
    value: Option<&str>,
    devices: &[DeviceInfo],
    default_label: &str,
) -> Option<Option<String>> {
    let mut changed = None;
    let missing = value.is_some_and(|v| !devices.iter().any(|d| d.id == v));
    let selected_text = match value {
        None => default_label.to_owned(),
        Some(v) => {
            if let Some(d) = devices.iter().find(|d| d.id == v) {
                if d.is_default {
                    format!("{} (default)", d.label)
                } else {
                    d.label.clone()
                }
            } else {
                format!("{v} (not connected)")
            }
        }
    };

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 7.0;
        ui.label(
            egui::RichText::new(label)
                .font(font_medium(13.0))
                .color(p.text),
        );
        let width = ui.available_width();
        egui::ComboBox::from_id_salt(id_salt)
            .width(width)
            .selected_text(
                egui::RichText::new(selected_text)
                    .font(font_body(14.0))
                    .color(p.text),
            )
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(value.is_none(), default_label)
                    .clicked()
                    && value.is_some()
                {
                    changed = Some(None);
                }
                for d in devices {
                    let text = if d.is_default {
                        format!("{} (default)", d.label)
                    } else {
                        d.label.clone()
                    };
                    let is_sel = value == Some(d.id.as_str());
                    if ui.selectable_label(is_sel, text).clicked() && !is_sel {
                        changed = Some(Some(d.id.clone()));
                    }
                }
                if missing && let Some(v) = value {
                    let _ = ui.selectable_label(true, format!("{v} (not connected)"));
                }
            });
    });

    changed
}

/// Styled single-line text input matching Kavox's rounded surface inputs.
pub fn text_input(
    ui: &mut Ui,
    p: &Palette,
    text: &mut String,
    placeholder: &str,
    width: f32,
    char_limit: usize,
) -> Response {
    let mut resp = None;
    Frame::NONE
        .fill(p.surface)
        .corner_radius(CornerRadius::same(RADIUS_FIELD))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            let r = ui.add_sized(
                Vec2::new((width - 24.0).max(40.0), 20.0),
                TextEdit::singleline(text)
                    .hint_text(egui::RichText::new(placeholder).color(p.muted))
                    .font(font_body(14.0))
                    .frame(Frame::NONE)
                    .char_limit(char_limit),
            );
            resp = Some(r);
        });
    resp.unwrap()
}

pub fn callout(ui: &mut Ui, p: &Palette, warn: bool, add_contents: impl FnOnce(&mut Ui)) {
    let bg = if warn {
        mix(p.amber, p.panel, 0.12)
    } else {
        p.surface
    };
    Frame::NONE
        .fill(bg)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
}

/// Modal dialog with scrim overlay; returns `true` if user clicked the scrim to close.
pub fn modal(
    ctx: &egui::Context,
    p: &Palette,
    id: &str,
    max_width: f32,
    fixed_height: Option<f32>,
    add_contents: impl FnOnce(&mut Ui),
) -> bool {
    let screen = ctx.content_rect();
    let mut clicked_outside = false;

    egui::Area::new(Id::unique(id).with("scrim"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (scrim_rect, scrim_resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(scrim_rect, CornerRadius::ZERO, p.scrim);
            if scrim_resp.clicked() {
                clicked_outside = true;
            }
        });

    let avail_w = (screen.width() - 32.0).max(280.0);
    let avail_h = (screen.height() - 32.0).max(240.0);
    let dialog_w = max_width.min(avail_w);
    let dialog_h = fixed_height.map(|h| h.min(avail_h));

    egui::Area::new(Id::unique(id).with("dialog"))
        .order(Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            let frame = Frame::NONE
                .fill(p.panel)
                .stroke(Stroke::new(1.0, p.line))
                .corner_radius(CornerRadius::same(RADIUS_PANEL))
                .shadow(egui::Shadow {
                    offset: [0, 22],
                    blur: 56,
                    spread: 0,
                    color: p.shadow,
                });
            frame.show(ui, |ui| {
                ui.set_width(dialog_w);
                if let Some(h) = dialog_h {
                    ui.set_height(h);
                } else {
                    ui.set_max_height(avail_h);
                }
                add_contents(ui);
            });
        });

    clicked_outside
}
