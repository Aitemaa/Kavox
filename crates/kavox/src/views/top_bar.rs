//! Top navigation bar: LogoMark + Wordmark, Sounds/Voice switcher, BoardTabs, Search, Add sounds, Settings.

use eframe::egui::{
    self, Align, CornerRadius, CursorIcon, Frame, Id, Key, Layout, Margin, Order, Pos2, Rect,
    ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
use kavox_core::Board;

use crate::icons::{IconName, draw_logo_mark, draw_wordmark, paint_icon};
use crate::state::{SettingsTab, UiState, View};
use crate::theme::{Palette, RADIUS_FIELD, font_body, font_medium};
use crate::views::ActionCtx;
use crate::widgets::{ButtonVariant, icon_button, pill_button, segmented};

pub fn show(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    let narrow = ui.available_width() < 720.0;
    let voice_live =
        state.settings.voice.enabled && state.settings.voice.preset != crate::voices::NORMAL;

    Frame::NONE
        .fill(p.bg)
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if narrow {
                // Row 1: Brand + View + Actions
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    draw_brand(ui, p);
                    draw_view_switcher(ui, p, state, voice_live);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        draw_actions(ui, p, state, actions, true);
                    });
                });
                if state.view == View::Sounds {
                    ui.add_space(8.0);
                    draw_board_tabs(ui, p, state, actions);
                }
            } else {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 18.0;
                    draw_brand(ui, p);
                    draw_view_switcher(ui, p, state, voice_live);

                    // Reserve right-side space matching draw_actions so board tabs never collide.
                    let win_w = ui.ctx().content_rect().width();
                    let search_w = if win_w < 1020.0 { 145.0 } else { 200.0 };
                    let actions_w = if state.view == View::Sounds {
                        search_w + 132.0 + 34.0 + 16.0 + 18.0
                    } else {
                        44.0
                    };
                    let tabs_w = (ui.available_width() - actions_w).max(80.0);

                    ui.allocate_ui_with_layout(
                        Vec2::new(tabs_w, 36.0),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.set_min_height(36.0);
                            if state.view == View::Sounds {
                                draw_board_tabs(ui, p, state, actions);
                            }
                        },
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        draw_actions(ui, p, state, actions, false);
                    });
                });
            }
        });

    // Bottom 1px border line
    let rect = ui.min_rect();
    let y = rect.max.y;
    ui.painter().line_segment(
        [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
        Stroke::new(1.0, p.line),
    );

    // Floating board context menu (Rename / Delete)
    draw_board_menu_popup(ui.ctx(), p, state);
}

fn draw_brand(ui: &mut Ui, p: &Palette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        draw_logo_mark(ui, 28.0);
        draw_wordmark(ui, 16.0, p.text);
    });
}

fn draw_view_switcher(ui: &mut Ui, p: &Palette, state: &mut UiState, voice_live: bool) {
    let voice_label = if voice_live { "Voice · on" } else { "Voice" };
    if let Some(next) = segmented(
        ui,
        p,
        state.view,
        &[
            (View::Sounds, "Sounds", Some(IconName::Play)),
            (View::Voice, voice_label, Some(IconName::Voice)),
        ],
    ) {
        state.set_view(next);
    }
}

fn draw_actions(
    ui: &mut Ui,
    p: &Palette,
    state: &mut UiState,
    actions: &mut ActionCtx<'_>,
    narrow: bool,
) {
    // Remember: inside right_to_left layout, widgets are added right-to-left.
    ui.spacing_mut().item_spacing.x = 8.0;

    if icon_button(ui, p, IconName::Gear, 18.0, 34.0, "Settings").clicked() {
        state.open_settings(SettingsTab::Audio);
    }

    if state.view == View::Sounds {
        let add_resp = if narrow {
            pill_button(
                ui,
                p,
                "",
                Some(IconName::Plus),
                ButtonVariant::Primary,
                true,
            )
            .on_hover_text("Add sounds")
        } else {
            pill_button(
                ui,
                p,
                "Add sounds",
                Some(IconName::Plus),
                ButtonVariant::Primary,
                true,
            )
        };
        if add_resp.clicked() {
            actions.pick_and_import(state);
        }

        let search_w = if narrow {
            120.0
        } else if ui.ctx().content_rect().width() < 1020.0 {
            145.0
        } else {
            200.0
        };
        draw_search_box(ui, p, state, search_w);
    }
}

fn draw_search_box(ui: &mut Ui, p: &Palette, state: &mut UiState, width: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::hover());
    let hovered =
        rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or(Pos2::new(-1.0, -1.0))));
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(18), p.surface);
    if hovered {
        painter.rect_stroke(
            rect,
            CornerRadius::same(18),
            Stroke::new(1.0, p.line_strong),
            StrokeKind::Inside,
        );
    }
    paint_icon(
        painter,
        Pos2::new(rect.min.x + 20.0, rect.center().y),
        16.0,
        IconName::Search,
        p.muted,
    );

    let has_text = !state.search.is_empty();
    let right_pad = if has_text { 28.0 } else { 12.0 };
    let text_rect = Rect::from_min_max(
        Pos2::new(rect.min.x + 36.0, rect.min.y + 8.0),
        Pos2::new(rect.max.x - right_pad, rect.max.y - 8.0),
    );

    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
    let edit_resp = child.add_sized(
        text_rect.size(),
        TextEdit::singleline(&mut state.search)
            .id(Id::unique("sound-search"))
            .hint_text(egui::RichText::new("Find a sound").color(p.muted))
            .font(font_body(14.0))
            .frame(Frame::NONE),
    );
    if state.focus_search {
        edit_resp.request_focus();
        state.focus_search = false;
    }
    if edit_resp.has_focus() {
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            state.search.clear();
            edit_resp.surrender_focus();
        } else if ui.input(|i| i.key_pressed(Key::Enter)) {
            if let Some(board) = state.active_board() {
                let q = state.search.trim().to_lowercase();
                if let Some(first_id) = board.sounds.iter().find(|id| {
                    state
                        .library
                        .sounds
                        .get(*id)
                        .is_some_and(|s| q.is_empty() || s.name.to_lowercase().contains(&q))
                }) {
                    state.focused_pad = Some(first_id.clone());
                }
            }
            edit_resp.surrender_focus();
        }
    }

    if has_text {
        let clear_rect = Rect::from_center_size(
            Pos2::new(rect.max.x - 16.0, rect.center().y),
            Vec2::splat(20.0),
        );
        let clear_resp = ui.interact(clear_rect, Id::unique("search-clear"), Sense::click());
        if clear_resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        paint_icon(
            ui.painter(),
            clear_rect.center(),
            13.0,
            IconName::Close,
            if clear_resp.hovered() {
                p.text
            } else {
                p.muted
            },
        );
        if clear_resp.clicked() {
            state.search.clear();
        }
    }
}

fn draw_board_tabs(ui: &mut Ui, p: &Palette, state: &mut UiState, actions: &mut ActionCtx<'_>) {
    let boards: Vec<Board> = state.library.boards.clone();
    let active_id = state.library.active_board.clone();

    ScrollArea::horizontal()
        .id_salt("board-tabs-scroll")
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;

                for board in &boards {
                    let is_active = board.id == active_id;
                    if state.renaming_board.as_deref() == Some(board.id.as_str()) {
                        if let Some(committed) = draw_board_name_input(
                            ui,
                            p,
                            "rename-board-input",
                            &mut state.board_name_draft,
                            &mut state.board_name_focus,
                            "",
                        ) {
                            state.renaming_board = None;
                            let trimmed = committed.trim();
                            if !trimmed.is_empty() && trimmed != board.name {
                                let res =
                                    actions.controller.lock().rename_board(&board.id, trimmed);
                                actions.run_or_toast(state, res);
                            }
                        }
                        continue;
                    }

                    let name_font = font_medium(14.0);
                    let count_font = font_body(12.0);
                    let name_g =
                        ui.painter()
                            .layout_no_wrap(board.name.clone(), name_font.clone(), p.text);
                    let count_str = board.sounds.len().to_string();
                    let count_g =
                        ui.painter()
                            .layout_no_wrap(count_str.clone(), count_font.clone(), p.faint);

                    let right_pad = if is_active { 4.0 } else { 10.0 };
                    let tab_w =
                        10.0 + 6.0 + 8.0 + name_g.size().x + 8.0 + count_g.size().x + right_pad;
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(tab_w, 34.0), Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if ui.is_rect_visible(rect) {
                        let painter = ui.painter();
                        let cy = rect.center().y;
                        let dot_x = rect.min.x + 13.0;
                        if is_active {
                            painter.circle_filled(Pos2::new(dot_x, cy), 3.0, p.red);
                        }
                        let text_col = if is_active || resp.hovered() {
                            p.text
                        } else {
                            p.muted
                        };
                        let count_col = if is_active { p.muted } else { p.faint };
                        let ng = painter.layout_no_wrap(board.name.clone(), name_font, text_col);
                        let cg = painter.layout_no_wrap(count_str, count_font, count_col);
                        let mut x = rect.min.x + 24.0;
                        painter.galley(Pos2::new(x, cy - ng.size().y * 0.5), ng, text_col);
                        x += name_g.size().x + 8.0;
                        painter.galley(Pos2::new(x, cy - cg.size().y * 0.5), cg, count_col);
                    }

                    if resp.double_clicked() {
                        state.renaming_board = Some(board.id.clone());
                        state.board_name_draft = board.name.clone();
                        state.board_name_focus = true;
                        state.board_menu_id = None;
                    } else if resp.clicked() && !is_active {
                        let res = actions.controller.lock().set_active_board(&board.id);
                        actions.run_or_toast(state, res);
                        state.board_menu_id = None;
                    }

                    if is_active {
                        let menu_resp = icon_button(
                            ui,
                            p,
                            IconName::Dots,
                            14.0,
                            26.0,
                            &format!("{} options", board.name),
                        );
                        if menu_resp.clicked() {
                            if state.board_menu_id.as_deref() == Some(board.id.as_str()) {
                                state.board_menu_id = None;
                            } else {
                                state.board_menu_id = Some(board.id.clone());
                                state.board_menu_pos = Pos2::new(
                                    (menu_resp.rect.max.x - 200.0).max(8.0),
                                    menu_resp.rect.max.y + 6.0,
                                );
                            }
                        }
                    }
                }

                if state.creating_board {
                    if let Some(committed) = draw_board_name_input(
                        ui,
                        p,
                        "create-board-input",
                        &mut state.board_name_draft,
                        &mut state.board_name_focus,
                        "New board",
                    ) {
                        state.creating_board = false;
                        let trimmed = committed.trim();
                        if !trimmed.is_empty() {
                            let res = actions.controller.lock().create_board(trimmed);
                            if let Some(created) = actions.run_or_toast(state, res) {
                                let res2 = actions.controller.lock().set_active_board(&created.id);
                                actions.run_or_toast(state, res2);
                            }
                        }
                    }
                } else {
                    ui.add_space(4.0);
                    if icon_button(ui, p, IconName::Plus, 16.0, 34.0, "New board").clicked() {
                        state.creating_board = true;
                        state.renaming_board = None;
                        state.board_name_draft.clear();
                        state.board_name_focus = true;
                        state.board_menu_id = None;
                    }
                }
            });
        });
}

fn draw_board_name_input(
    ui: &mut Ui,
    p: &Palette,
    id: &str,
    draft: &mut String,
    should_focus: &mut bool,
    placeholder: &str,
) -> Option<String> {
    let mut done = None;
    Frame::NONE
        .fill(p.surface)
        .corner_radius(CornerRadius::same(RADIUS_FIELD))
        .stroke(Stroke::new(1.0, p.line_strong))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            let resp = ui.add_sized(
                Vec2::new(130.0, 20.0),
                TextEdit::singleline(draft)
                    .id(Id::unique(id))
                    .hint_text(egui::RichText::new(placeholder).color(p.muted))
                    .font(font_body(14.0))
                    .frame(Frame::NONE)
                    .char_limit(60),
            );
            if *should_focus {
                resp.request_focus();
                *should_focus = false;
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                done = Some(String::new());
            } else if ui.input(|i| i.key_pressed(Key::Enter)) || resp.lost_focus() {
                done = Some(draft.clone());
            }
        });
    done
}

fn draw_board_menu_popup(ctx: &egui::Context, p: &Palette, state: &mut UiState) {
    let Some(board_id) = state.board_menu_id.clone() else {
        return;
    };
    let Some(board) = state
        .library
        .boards
        .iter()
        .find(|b| b.id == board_id)
        .cloned()
    else {
        state.board_menu_id = None;
        return;
    };
    let can_delete = state.library.boards.len() > 1;

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.board_menu_id = None;
        return;
    }

    let area_resp = egui::Area::new(Id::unique("board-context-menu"))
        .order(Order::Foreground)
        .fixed_pos(state.board_menu_pos)
        .show(ctx, |ui| {
            Frame::NONE
                .fill(p.panel)
                .stroke(Stroke::new(1.0, p.line))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::same(5))
                .shadow(egui::Shadow {
                    offset: [0, 14],
                    blur: 36,
                    spread: 0,
                    color: p.shadow,
                })
                .show(ui, |ui| {
                    ui.set_width(190.0);
                    ui.spacing_mut().item_spacing.y = 2.0;

                    if menu_item(ui, p, IconName::Pencil, "Rename board", false, true).clicked() {
                        state.board_menu_id = None;
                        state.renaming_board = Some(board.id.clone());
                        state.board_name_draft = board.name.clone();
                        state.board_name_focus = true;
                    }

                    let del_resp =
                        menu_item(ui, p, IconName::Trash, "Delete board", true, can_delete);
                    let del_resp = if !can_delete {
                        del_resp.on_hover_text("Kavox needs at least one board")
                    } else {
                        del_resp
                    };
                    if can_delete && del_resp.clicked() {
                        state.board_menu_id = None;
                        state.confirm_delete_board(&board);
                    }
                });
        });

    if ctx.input(|i| i.pointer.any_pressed())
        && !area_resp
            .response
            .rect
            .contains(ctx.input(|i| i.pointer.interact_pos().unwrap_or(Pos2::new(-1.0, -1.0))))
    {
        state.board_menu_id = None;
    }
}

fn menu_item(
    ui: &mut Ui,
    p: &Palette,
    icon: IconName,
    label: &str,
    danger: bool,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), sense);
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if enabled && resp.hovered() {
            painter.rect_filled(rect, CornerRadius::same(9), p.surface);
        }
        let mut fg = if danger { p.red_text } else { p.text };
        if !enabled {
            fg = fg.gamma_multiply(0.45);
        }
        let cy = rect.center().y;
        paint_icon(painter, Pos2::new(rect.min.x + 17.5, cy), 15.0, icon, fg);
        let g = painter.layout_no_wrap(label.to_owned(), font_body(14.0), fg);
        painter.galley(Pos2::new(rect.min.x + 35.0, cy - g.size().y * 0.5), g, fg);
    }
    resp
}
