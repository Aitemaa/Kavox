//! Responsive Soundboard PadGrid with drag-to-reorder, keyboard navigation, hit flash, progress bar, and Stop pill.

use std::time::{Duration, Instant};

use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, Key, Pos2, Rect, ScrollArea, Sense, Stroke,
    StrokeKind, Ui, Vec2, text::LayoutJob,
};
use kavox_core::{Retrigger, Sound};

use crate::hotkey;
use crate::icons::{IconName, paint_icon};
use crate::state::{UiState, format_duration};
use crate::theme::{
    Palette, RADIUS_PAD, font_body, font_heading, font_medium, mix, parse_hex, with_alpha,
};
use crate::views::ActionCtx;
use crate::widgets::{ButtonVariant, paint_keycaps_at, pill_button};

pub fn show(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    let Some(board) = state.active_board().cloned() else {
        return;
    };

    let all: Vec<Sound> = board
        .sounds
        .iter()
        .filter_map(|id| state.library.sounds.get(id).cloned())
        .collect();

    let query = state.search.trim().to_lowercase();
    let sounds: Vec<Sound> = if query.is_empty() {
        all.clone()
    } else {
        all.iter()
            .filter(|s| s.name.to_lowercase().contains(&query))
            .cloned()
            .collect()
    };

    let narrow = ui.ctx().content_rect().width() < 720.0;
    let pad_x = if narrow { 16.0 } else { 20.0 };
    let pad_top = if narrow { 14.0 } else { 20.0 };

    ScrollArea::vertical()
        .id_salt("pad-grid-scroll")
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
                    ui.set_width(ui.available_width());

                    if all.is_empty() {
                        show_empty_board(ui, p, &board.name, state, actions);
                        return;
                    }
                    if sounds.is_empty() {
                        ui.add_space(24.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "No sounds on {} match \"{}\".",
                                board.name,
                                state.search.trim()
                            ))
                            .font(font_body(14.0))
                            .color(p.muted),
                        );
                        return;
                    }

                    let gap = if narrow { 10.0 } else { 12.0 };
                    let min_tile_w = if narrow { 132.0 } else { 156.0 };
                    let avail_w = ui.available_width().max(min_tile_w);
                    let cols = (((avail_w + gap) / (min_tile_w + gap)).floor() as usize).max(1);
                    let tile_w = (avail_w - gap * (cols as f32 - 1.0)) / cols as f32;
                    let tile_h = if narrow {
                        104.0f32
                    } else {
                        (tile_w * 4.0 / 5.0).max(112.0)
                    };

                    // Keyboard grid navigation when no dialog/text input has focus.
                    if state.dialog.is_none()
                        && state.confirm.is_none()
                        && state.recording_hotkey.is_none()
                        && !ui.ctx().egui_wants_keyboard_input()
                    {
                        handle_grid_keyboard(
                            ui,
                            state,
                            actions,
                            &board.id,
                            &sounds,
                            cols,
                            query.is_empty(),
                        );
                    }

                    let show_add = query.is_empty();
                    let total_items = sounds.len() + usize::from(show_add);
                    let rows = total_items.div_ceil(cols);

                    let mut slot_rects: Vec<(String, Rect)> = Vec::with_capacity(sounds.len());
                    let mut reorder_request: Option<(String, usize)> = None;

                    for row in 0..rows {
                        if row > 0 {
                            ui.add_space(gap);
                        }
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for col in 0..cols {
                                let idx = row * cols + col;
                                if idx < sounds.len() {
                                    let sound = &sounds[idx];
                                    let rect = draw_pad(
                                        ui,
                                        p,
                                        state,
                                        actions,
                                        sound,
                                        Vec2::new(tile_w, tile_h),
                                        query.is_empty(),
                                    );
                                    slot_rects.push((sound.id.clone(), rect));
                                } else if idx == sounds.len() && show_add {
                                    draw_add_pad(ui, p, state, actions, Vec2::new(tile_w, tile_h));
                                }
                            }
                        });
                    }

                    // Complete or cancel pointer drag-to-reorder.
                    if let Some(dragged_id) = state.dragging_pad.clone() {
                        if ui.input(|i| i.pointer.primary_down()) {
                            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                                if !state.drag_active && pos.distance(state.drag_origin) >= 6.0 {
                                    state.drag_active = true;
                                }
                                if state.drag_active {
                                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                                    // Highlight target drop slot
                                    if let Some((_, target_rect)) = slot_rects
                                        .iter()
                                        .find(|(id, r)| id != &dragged_id && r.contains(pos))
                                    {
                                        ui.painter().rect_stroke(
                                            *target_rect,
                                            CornerRadius::same(RADIUS_PAD),
                                            Stroke::new(2.0, p.focus),
                                            StrokeKind::Inside,
                                        );
                                    }
                                }
                            }
                        } else {
                            if state.drag_active
                                && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
                                && let Some((target_id, _)) = slot_rects
                                    .iter()
                                    .find(|(id, r)| id != &dragged_id && r.contains(pos))
                                && let Some(to_idx) =
                                    board.sounds.iter().position(|id| id == target_id)
                            {
                                reorder_request = Some((dragged_id, to_idx));
                            }
                            state.dragging_pad = None;
                            state.drag_active = false;
                        }
                    }

                    if let Some((id, to_idx)) = reorder_request {
                        move_sound(state, actions, &board.id, &id, to_idx);
                    }
                });
        });
}

#[allow(clippy::too_many_arguments)]
fn draw_pad(
    ui: &mut Ui,
    p: &Palette,
    state: &mut UiState,
    actions: &mut ActionCtx<'_>,
    sound: &Sound,
    size: Vec2,
    can_drag: bool,
) -> Rect {
    let voices = state.playing.get(&sound.id);
    let playing = voices.is_some_and(|v| !v.is_empty());
    let progress = voices
        .and_then(|v| v.last())
        .and_then(|v| (v.frames > 0).then_some((v.frame as f32 / v.frames as f32).clamp(0.0, 1.0)))
        .unwrap_or(0.0);

    let is_dragging_this =
        state.drag_active && state.dragging_pad.as_deref() == Some(sound.id.as_str());
    let is_focused = state.focused_pad.as_deref() == Some(sound.id.as_str());

    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let pointer_pos = ui.input(|i| i.pointer.hover_pos());
    let hovered = pointer_pos.is_some_and(|pos| rect.contains(pos));

    // Edit corner button rect (top-right 28x24)
    let edit_rect = Rect::from_min_size(
        Pos2::new(rect.max.x - 35.0, rect.min.y + 7.0),
        Vec2::new(28.0, 24.0),
    );
    let edit_resp = ui.interact(
        edit_rect,
        ui.make_persistent_id(("pad-edit", &sound.id)),
        Sense::click(),
    );

    // Stop pill rect (bottom-right while playing)
    let stop_rect = Rect::from_min_size(
        Pos2::new(rect.max.x - 68.0, rect.max.y - 34.0),
        Vec2::new(59.0, 24.0),
    );
    let stop_resp = if playing {
        Some(ui.interact(
            stop_rect,
            ui.make_persistent_id(("pad-stop", &sound.id)),
            Sense::click(),
        ))
    } else {
        None
    };

    let over_edit = edit_resp.hovered();
    let over_stop = stop_resp.as_ref().is_some_and(|r| r.hovered());

    if hovered || over_edit || over_stop {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    // Drag start tracking
    if can_drag
        && response.drag_started()
        && !over_edit
        && !over_stop
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.dragging_pad = Some(sound.id.clone());
        state.drag_origin = pos;
        state.drag_active = false;
    }

    // Click actions
    if edit_resp.clicked() || response.secondary_clicked() {
        state.start_editing(sound.id.clone());
    } else if stop_resp.as_ref().is_some_and(|r| r.clicked()) {
        actions.controller.lock().stop_sound(&sound.id);
    } else if response.clicked() && !state.drag_active && !over_edit && !over_stop {
        state.focused_pad = Some(sound.id.clone());
        state.pad_hit_at.insert(sound.id.clone(), Instant::now());
        let res = actions.controller.lock().play_sound(&sound.id);
        actions.run_or_toast(state, res);
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let draw_rect = if response.is_pointer_button_down_on()
            && !over_edit
            && !over_stop
            && !state.drag_active
        {
            rect.translate(Vec2::new(0.0, 1.0))
        } else {
            rect
        };

        let alpha = if is_dragging_this { 0.85 } else { 1.0 };
        let bg = if playing || (hovered && !over_edit && !over_stop) {
            p.surface_2
        } else {
            p.surface
        }
        .gamma_multiply(alpha);

        let border = if is_focused {
            Stroke::new(2.0, p.focus)
        } else if playing {
            Stroke::new(1.0, with_alpha(p.text, 72))
        } else {
            Stroke::NONE
        };

        painter.rect_filled(draw_rect, CornerRadius::same(RADIUS_PAD), bg);
        if border != Stroke::NONE {
            painter.rect_stroke(
                draw_rect,
                CornerRadius::same(RADIUS_PAD),
                border,
                StrokeKind::Inside,
            );
        }

        // Pad colour dot (top-left, 8px)
        let pad_color = parse_hex(&sound.color);
        let dot_center = Pos2::new(draw_rect.min.x + 19.0, draw_rect.min.y + 19.0);
        if playing {
            painter.circle_filled(dot_center, 8.0, with_alpha(pad_color, 56));
            painter.circle_filled(dot_center, 4.0, pad_color);
        } else {
            painter.circle_filled(dot_center, 4.0, pad_color.gamma_multiply(0.55));
        }

        // Pad name (34px top padding, 14px side padding, up to 3 lines)
        let name_rect = Rect::from_min_max(
            Pos2::new(draw_rect.min.x + 14.0, draw_rect.min.y + 34.0),
            Pos2::new(draw_rect.max.x - 14.0, draw_rect.max.y - 36.0),
        );
        let mut job = LayoutJob::single_section(
            sound.name.clone(),
            egui::TextFormat {
                font_id: font_medium(15.0),
                color: p.text.gamma_multiply(alpha),
                ..Default::default()
            },
        );
        job.wrap.max_width = name_rect.width().max(40.0);
        job.wrap.max_rows = 3;
        job.wrap.break_anywhere = true;
        let galley = painter.layout_job(job);
        painter.galley(name_rect.min, galley, p.text);

        // Footer row: hotkey keycaps (bottom-left) + mode icon & duration (bottom-right when not playing)
        let foot_y = draw_rect.max.y - 21.0;
        if let Some(hk) = &sound.hotkey {
            let caps = hotkey::keycaps(hk, state.platform);
            paint_keycaps_at(
                painter,
                ui,
                p,
                Pos2::new(draw_rect.min.x + 14.0, foot_y),
                &caps,
                true,
            );
        }

        if !playing {
            let dur_str = format_duration(sound.duration_ms);
            let dur_g = painter.layout_no_wrap(dur_str, font_body(12.0), p.muted);
            let dur_x = draw_rect.max.x - 14.0 - dur_g.size().x;
            painter.galley(
                Pos2::new(dur_x, foot_y - dur_g.size().y * 0.5),
                dur_g,
                p.muted,
            );

            let mode_icon = if sound.looping {
                Some(IconName::Loop)
            } else {
                match sound.retrigger {
                    Retrigger::Toggle => Some(IconName::Toggle),
                    Retrigger::Overlap => Some(IconName::Overlap),
                    Retrigger::Restart => None,
                }
            };
            if let Some(ic) = mode_icon {
                paint_icon(painter, Pos2::new(dur_x - 12.5, foot_y), 13.0, ic, p.muted);
            }
        }

        // Bottom 2px progress bar while playing (`1 - progress` like CSS)
        if playing {
            let rem = (1.0 - progress).clamp(0.0, 1.0);
            if rem > 0.002 {
                let bar_w = draw_rect.width() * rem;
                let bar_rect = Rect::from_min_size(
                    Pos2::new(draw_rect.min.x, draw_rect.max.y - 2.5),
                    Vec2::new(bar_w, 2.5),
                );
                let clipped = painter.with_clip_rect(draw_rect);
                clipped.rect_filled(bar_rect, CornerRadius::ZERO, p.lit);
            }
        }

        // Hit flash overlay (300ms ease-out from 0.10 to 0)
        if let Some(hit_at) = state.pad_hit_at.get(&sound.id).copied() {
            let elapsed = hit_at.elapsed();
            if elapsed < Duration::from_millis(300) {
                let t = 1.0 - (elapsed.as_secs_f32() / 0.30);
                let flash_alpha = (t * t * 0.12 * 255.0).round() as u8;
                painter.rect_filled(
                    draw_rect,
                    CornerRadius::same(RADIUS_PAD),
                    with_alpha(p.text, flash_alpha),
                );
                ui.ctx().request_repaint();
            } else {
                state.pad_hit_at.remove(&sound.id);
            }
        }

        // Hover edit corner button (`...`)
        if hovered || over_edit || state.editing.as_deref() == Some(sound.id.as_str()) {
            if over_edit {
                painter.rect_filled(edit_rect, CornerRadius::same(12), with_alpha(p.text, 26));
            }
            paint_icon(
                painter,
                edit_rect.center(),
                16.0,
                IconName::Dots,
                if over_edit { p.text } else { p.muted },
            );
        }

        // Playing Stop pill button
        if playing {
            let stop_bg = if over_stop { p.lit } else { p.surface_2 };
            let stop_fg = if over_stop { p.on_lit } else { p.text };
            painter.rect_filled(stop_rect, CornerRadius::same(12), stop_bg);
            if !over_stop {
                painter.rect_stroke(
                    stop_rect,
                    CornerRadius::same(12),
                    Stroke::new(1.0, p.line_strong),
                    StrokeKind::Inside,
                );
            }
            paint_icon(
                painter,
                Pos2::new(stop_rect.min.x + 15.0, stop_rect.center().y),
                11.0,
                IconName::Stop,
                stop_fg,
            );
            let sg = painter.layout_no_wrap("Stop".into(), font_medium(12.0), stop_fg);
            painter.galley(
                Pos2::new(
                    stop_rect.min.x + 25.0,
                    stop_rect.center().y - sg.size().y * 0.5,
                ),
                sg,
                stop_fg,
            );
        }
    }

    rect
}

fn draw_add_pad(
    ui: &mut Ui,
    p: &Palette,
    state: &UiState,
    actions: &mut ActionCtx<'_>,
    size: Vec2,
) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let border_col = if resp.hovered() {
            p.muted
        } else {
            p.line_strong
        };
        let fg = if resp.hovered() { p.text } else { p.muted };
        draw_dashed_rounded_rect(
            painter,
            rect,
            RADIUS_PAD as f32,
            Stroke::new(1.0, border_col),
        );

        let center = rect.center();
        paint_icon(
            painter,
            Pos2::new(center.x, center.y - 12.0),
            22.0,
            IconName::Plus,
            fg,
        );
        let g = painter.layout_no_wrap("Add sounds".into(), font_medium(14.0), fg);
        painter.galley(
            Pos2::new(center.x - g.size().x * 0.5, center.y + 6.0),
            g,
            fg,
        );
    }
    if resp.clicked() {
        actions.pick_and_import(state);
    }
}

fn show_empty_board(
    ui: &mut Ui,
    p: &Palette,
    board_name: &str,
    state: &UiState,
    actions: &mut ActionCtx<'_>,
) {
    let top_space = (ui.ctx().content_rect().height() * 0.07).max(24.0);
    ui.add_space(top_space);
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 12.0;

        // 8-pad illustration (4x2), pad #3 lit with red dot
        let cell_w = 54.0;
        let cell_h = 44.0;
        let gap = 8.0;
        let grid_w = cell_w * 4.0 + gap * 3.0;
        let grid_h = cell_h * 2.0 + gap;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(grid_w, grid_h), Sense::hover());
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            for i in 0..8 {
                let col = (i % 4) as f32;
                let row = (i / 4) as f32;
                let r = Rect::from_min_size(
                    Pos2::new(rect.min.x + col * (cell_w + gap), rect.min.y + row * (cell_h + gap)),
                    Vec2::new(cell_w, cell_h),
                );
                let is_lit = i == 2;
                if is_lit {
                    painter.rect_filled(r, CornerRadius::same(11), p.surface);
                } else {
                    painter.rect_stroke(r, CornerRadius::same(11), Stroke::new(1.0, p.line), StrokeKind::Inside);
                }
                let dot = Pos2::new(r.min.x + 12.0, r.min.y + 12.0);
                painter.circle_filled(dot, 3.0, if is_lit { p.red } else { p.line_strong });
            }
        }

        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(format!("{board_name} has no sounds yet"))
                .font(font_heading(20.0))
                .color(p.text),
        );

        ui.allocate_ui_with_layout(
            Vec2::new(420.0f32.min(ui.available_width()), 0.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new("Drop audio files anywhere in this window, or pick them from your computer. WAV, MP3, OGG and FLAC all work.")
                        .font(font_body(14.0))
                        .color(p.muted),
                );
            },
        );

        ui.add_space(6.0);
        if pill_button(
            ui,
            p,
            "Add sounds",
            Some(IconName::Plus),
            ButtonVariant::Primary,
            true,
        )
        .clicked()
        {
            actions.pick_and_import(state);
        }
    });
}

fn handle_grid_keyboard(
    ui: &Ui,
    state: &mut UiState,
    actions: &mut ActionCtx<'_>,
    board_id: &str,
    sounds: &[Sound],
    cols: usize,
    can_reorder: bool,
) {
    let Some(focused_id) = state.focused_pad.clone() else {
        return;
    };
    let Some(idx) = sounds.iter().position(|s| s.id == focused_id) else {
        return;
    };

    let (arrow, alt, play, edit, delete) = ui.input(|i| {
        let arrow = if i.key_pressed(Key::ArrowLeft) {
            Some(-1isize)
        } else if i.key_pressed(Key::ArrowRight) {
            Some(1)
        } else if i.key_pressed(Key::ArrowUp) {
            Some(-(cols as isize))
        } else if i.key_pressed(Key::ArrowDown) {
            Some(cols as isize)
        } else {
            None
        };
        (
            arrow,
            i.modifiers.alt,
            i.key_pressed(Key::Enter) || i.key_pressed(Key::Space),
            i.key_pressed(Key::F2),
            i.key_pressed(Key::Delete),
        )
    });

    if let Some(delta) = arrow {
        let next = idx as isize + delta;
        if next >= 0 && (next as usize) < sounds.len() {
            let next_id = sounds[next as usize].id.clone();
            if alt && can_reorder {
                if let Some(board) = state.library.boards.iter().find(|b| b.id == board_id)
                    && let Some(target_idx) = board.sounds.iter().position(|id| id == &next_id)
                {
                    move_sound(state, actions, board_id, &focused_id, target_idx);
                }
            } else {
                state.focused_pad = Some(next_id);
            }
        }
    } else if play {
        state.pad_hit_at.insert(focused_id.clone(), Instant::now());
        let res = actions.controller.lock().play_sound(&focused_id);
        actions.run_or_toast(state, res);
    } else if edit {
        state.start_editing(focused_id);
    } else if delete {
        state.confirm_delete_sound(&focused_id);
    }
}

fn move_sound(
    state: &mut UiState,
    actions: &mut ActionCtx<'_>,
    board_id: &str,
    sound_id: &str,
    to_idx: usize,
) {
    let Some(board) = state.library.boards.iter_mut().find(|b| b.id == board_id) else {
        return;
    };
    let Some(from_idx) = board.sounds.iter().position(|id| id == sound_id) else {
        return;
    };
    let target = to_idx.min(board.sounds.len().saturating_sub(1));
    if from_idx == target {
        return;
    }
    let item = board.sounds.remove(from_idx);
    board.sounds.insert(target, item);
    let ids = board.sounds.clone();
    let res = actions.controller.lock().reorder_sounds(board_id, ids);
    actions.run_or_toast(state, res);
}

/// Draws a clean dashed rounded rectangle (used by `.pad-add` and `.drop-target`).
pub fn draw_dashed_rounded_rect(painter: &egui::Painter, rect: Rect, radius: f32, stroke: Stroke) {
    let r = radius.min(rect.width() * 0.5).min(rect.height() * 0.5);
    let mut pts = Vec::with_capacity(64);
    let corners = [
        (
            Pos2::new(rect.max.x - r, rect.min.y + r),
            -std::f32::consts::FRAC_PI_2,
            0.0,
        ),
        (
            Pos2::new(rect.max.x - r, rect.max.y - r),
            0.0,
            std::f32::consts::FRAC_PI_2,
        ),
        (
            Pos2::new(rect.min.x + r, rect.max.y - r),
            std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI,
        ),
        (
            Pos2::new(rect.min.x + r, rect.min.y + r),
            std::f32::consts::PI,
            1.5 * std::f32::consts::PI,
        ),
    ];
    for (center, start_a, end_a) in corners {
        let steps = 6;
        for s in 0..=steps {
            let a = start_a + (end_a - start_a) * (s as f32 / steps as f32);
            pts.push(Pos2::new(center.x + r * a.cos(), center.y + r * a.sin()));
        }
    }
    if let Some(&first) = pts.first() {
        pts.push(first);
    }

    let dash_len = 6.0;
    let gap_len = 5.0;
    let mut drawing = true;
    let mut rem = dash_len;

    for pair in pts.windows(2) {
        let mut a = pair[0];
        let b = pair[1];
        let mut seg_len = a.distance(b);
        if seg_len < 1e-4 {
            continue;
        }
        let dir = (b - a) / seg_len;
        while seg_len > 0.0 {
            let take = seg_len.min(rem);
            let next = a + dir * take;
            if drawing {
                painter.line_segment([a, next], stroke);
            }
            a = next;
            seg_len -= take;
            rem -= take;
            if rem <= 1e-4 {
                drawing = !drawing;
                rem = if drawing { dash_len } else { gap_len };
            }
        }
    }
}

#[allow(dead_code)]
fn _keep_mix_import(_: Color32) {
    let _ = mix;
}
