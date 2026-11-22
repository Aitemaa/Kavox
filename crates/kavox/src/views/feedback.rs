//! StatusBanner, Toasts, ConfirmDialog, and DropOverlay matching `src/components/Feedback.tsx`.

use eframe::egui::{
    self, Align, Align2, CornerRadius, Frame, Id, Key, Layout, Margin, Order, Pos2, Rect, Sense,
    Stroke, Ui, Vec2,
};
use kavox_core::{NoticeLevel, VirtualStatus};

use crate::icons::{IconName, paint_icon};
use crate::state::{ConfirmAction, Dialog, SettingsTab, UiState};
use crate::theme::{Palette, font_body, font_heading, mix};
use crate::views::{ActionCtx, pads::draw_dashed_rounded_rect};
use crate::widgets::{ButtonVariant, icon_button, modal, pill_button};

enum BannerAction {
    Wizard,
    AudioSettings,
    SoundSettings,
}

/// Top status banner shown only when sounds can't reach voice apps or config is read-only.
pub fn show_status_banner(
    ui: &mut Ui,
    p: &Palette,
    state: &mut UiState,
    actions: &mut ActionCtx<'_>,
) {
    let (is_warn, message, action) = match &state.virtual_status {
        VirtualStatus::Ok { .. } => {
            if !state.read_only {
                return;
            }
            (
                true,
                "Your sounds and settings were saved by a newer Kavox, so changes made here won’t be saved. Update Kavox to edit them.".to_owned(),
                None,
            )
        }
        VirtualStatus::NotInstalled => (
            false,
            "Voice apps can’t hear Kavox yet. Install the free VB-CABLE driver so Discord and games get a Kavox microphone.".to_owned(),
            Some(("Set up", BannerAction::Wizard)),
        ),
        VirtualStatus::Missing { device } => (
            false,
            format!("{device} isn’t connected, so sounds aren’t reaching your voice apps."),
            Some(("Choose another", BannerAction::AudioSettings)),
        ),
        VirtualStatus::CableIsDefaultOutput { device, .. } => (
            true,
            format!("{device} is your default speaker, so everything your PC plays goes into your mic. Pick your real speakers or headphones as the default."),
            if state.platform == "windows" {
                Some(("Open Sound settings", BannerAction::SoundSettings))
            } else {
                None
            },
        ),
        VirtualStatus::Failed { message } => (
            false,
            message.clone(),
            Some(("Audio settings", BannerAction::AudioSettings)),
        ),
    };

    let bg = if is_warn {
        mix(p.amber, p.bg, 0.09)
    } else {
        mix(p.red, p.bg, 0.09)
    };
    let icon_col = if is_warn { p.amber } else { p.red_text };

    Frame::NONE
        .fill(bg)
        .inner_margin(Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let (ir, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                paint_icon(ui.painter(), ir.center(), 18.0, IconName::Warning, icon_col);

                let btn_reserve = if action.is_some() { 180.0 } else { 0.0 };
                let text_w = (ui.available_width() - btn_reserve).max(120.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(text_w, 0.0),
                    Layout::top_down(Align::LEFT),
                    |ui| {
                        ui.label(
                            egui::RichText::new(message)
                                .font(font_body(14.0))
                                .color(p.text),
                        );
                    },
                );

                if let Some((label, kind)) = action {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if pill_button(ui, p, label, None, ButtonVariant::Default, true).clicked() {
                            match kind {
                                BannerAction::Wizard => {
                                    state.wizard_step = 0;
                                    state.dialog = Some(Dialog::Wizard);
                                }
                                BannerAction::AudioSettings => {
                                    state.open_settings(SettingsTab::Audio);
                                }
                                BannerAction::SoundSettings => {
                                    let res = crate::platform::open_sound_settings();
                                    actions.run_or_toast(state, res);
                                }
                            }
                        }
                    });
                }
            });
        });

    let br = ui.min_rect();
    ui.painter().line_segment(
        [Pos2::new(br.min.x, br.max.y), Pos2::new(br.max.x, br.max.y)],
        Stroke::new(1.0, p.line),
    );
}

/// Floating bottom-right toast stack.
pub fn show_toasts(ctx: &egui::Context, p: &Palette, state: &mut UiState) {
    state.prune_toasts();
    if state.toasts.is_empty() {
        return;
    }

    let screen = ctx.content_rect();
    let bottom_offset = if screen.width() < 720.0 { 150.0 } else { 92.0 };
    let toast_w = (screen.width() - 32.0).min(380.0);
    let toasts = state.toasts.clone();
    let mut dismiss_id = None;

    egui::Area::new(Id::unique("kavox-toasts"))
        .order(Order::Tooltip)
        .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -bottom_offset))
        .show(ctx, |ui| {
            ui.set_width(toast_w);
            ui.spacing_mut().item_spacing.y = 8.0;
            for t in &toasts {
                let (icon, icon_col) = match t.level {
                    NoticeLevel::Info => (IconName::Check, p.text),
                    NoticeLevel::Warn => (IconName::Warning, p.amber),
                    NoticeLevel::Error => (IconName::Info, p.red_text),
                };
                Frame::NONE
                    .fill(p.panel)
                    .stroke(Stroke::new(1.0, p.line))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(Margin {
                        left: 14,
                        right: 8,
                        top: 11,
                        bottom: 11,
                    })
                    .shadow(egui::Shadow {
                        offset: [0, 14],
                        blur: 36,
                        spread: 0,
                        color: p.shadow,
                    })
                    .show(ui, |ui| {
                        ui.set_width(toast_w - 22.0);
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            let (ir, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                            paint_icon(ui.painter(), ir.center(), 18.0, icon, icon_col);

                            let text_w = (ui.available_width() - 36.0).max(80.0);
                            ui.allocate_ui_with_layout(
                                Vec2::new(text_w, 0.0),
                                Layout::top_down(Align::LEFT),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(&t.message)
                                            .font(font_body(14.0))
                                            .color(p.text),
                                    );
                                },
                            );

                            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                if icon_button(ui, p, IconName::Close, 14.0, 26.0, "Dismiss")
                                    .clicked()
                                {
                                    dismiss_id = Some(t.id);
                                }
                            });
                        });
                    });
            }
        });

    if let Some(id) = dismiss_id {
        state.dismiss_toast(id);
    }

    // Wake up when the next toast expires.
    if let Some(next_exp) = state.toasts.iter().map(|t| t.expires_at).min() {
        let now = std::time::Instant::now();
        if next_exp > now {
            ctx.request_repaint_after(next_exp - now);
        }
    }
}

/// Small confirmation modal matching `ConfirmDialog`.
pub fn show_confirm_dialog(
    ctx: &egui::Context,
    p: &Palette,
    state: &mut UiState,
    actions: &mut ActionCtx<'_>,
) {
    let Some(confirm) = state.confirm.clone() else {
        return;
    };

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.confirm = None;
        return;
    }

    let mut cancel = false;
    let mut execute = false;

    let clicked_scrim = modal(ctx, p, "confirm-modal", 420.0, None, |ui| {
        Frame::NONE.inner_margin(Margin::same(22)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.label(
                egui::RichText::new(&confirm.title)
                    .font(font_heading(17.0))
                    .color(p.text),
            );
            ui.label(
                egui::RichText::new(&confirm.message)
                    .font(font_body(14.0))
                    .color(p.muted),
            );
        });

        let r = ui.min_rect();
        ui.painter().line_segment(
            [Pos2::new(r.min.x, r.max.y), Pos2::new(r.max.x, r.max.y)],
            Stroke::new(1.0, p.line),
        );

        Frame::NONE
            .inner_margin(Margin::symmetric(22, 14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if pill_button(
                        ui,
                        p,
                        &confirm.action_label,
                        None,
                        ButtonVariant::Danger,
                        true,
                    )
                    .clicked()
                    {
                        execute = true;
                    }
                    if pill_button(ui, p, "Cancel", None, ButtonVariant::Default, true).clicked() {
                        cancel = true;
                    }
                });
            });
    });

    if execute {
        state.confirm = None;
        match confirm.action {
            ConfirmAction::DeleteSound(id) => {
                let res = actions.controller.lock().delete_sound(&id);
                if actions.run_or_toast(state, res).is_some() {
                    actions.hotkeys.sync(actions.controller);
                }
            }
            ConfirmAction::DeleteBoard(id) => {
                let res = actions.controller.lock().delete_board(&id);
                if actions.run_or_toast(state, res).is_some() {
                    actions.hotkeys.sync(actions.controller);
                }
            }
        }
    } else if cancel || clicked_scrim {
        state.confirm = None;
    }
}

/// Fullscreen scrim shown while dragging audio files from the OS over the window.
pub fn show_drop_overlay(ctx: &egui::Context, p: &Palette, state: &UiState) {
    let hovering_files = ctx.input(|i| !i.raw.hovered_files.is_empty());
    if !hovering_files {
        return;
    }

    let board_name = state
        .active_board()
        .map(|b| b.name.as_str())
        .unwrap_or("Main");
    let screen = ctx.content_rect();

    egui::Area::new(Id::unique("drop-overlay"))
        .order(Order::Tooltip)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, CornerRadius::ZERO, p.scrim);

            let label = format!("Drop to add to {board_name}");
            let g = painter.layout_no_wrap(label, font_heading(18.0), p.text);
            let card_w = (g.size().x + 112.0).max(260.0);
            let card_h = 152.0;
            let card = Rect::from_center_size(rect.center(), Vec2::new(card_w, card_h));
            painter.rect_filled(card, CornerRadius::same(24), p.panel);
            draw_dashed_rounded_rect(painter, card, 24.0, Stroke::new(1.5, p.line_strong));

            paint_icon(
                painter,
                Pos2::new(card.center().x, card.center().y - 18.0),
                32.0,
                IconName::Plus,
                p.text,
            );
            painter.galley(
                Pos2::new(card.center().x - g.size().x * 0.5, card.center().y + 12.0),
                g,
                p.text,
            );
        });
}
