//! Exact Kavox colour tokens (dark + light themes) and Archivo typography matching `styles.css`.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals,
};
use kavox_core::Theme;

const ARCHIVO_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Archivo-Regular.ttf");
const ARCHIVO_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/Archivo-Medium.ttf");
const ARCHIVO_HEADING: &[u8] =
    include_bytes!("../../../assets/fonts/ArchivoSemiExpanded-SemiBold.ttf");

pub const FAMILY_MEDIUM: &str = "archivo-medium";
pub const FAMILY_HEADING: &str = "archivo-heading";

#[derive(Clone, Copy)]
pub struct Palette {
    pub is_dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub surface_2: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub lit: Color32,
    pub on_lit: Color32,
    pub red: Color32,
    pub red_text: Color32,
    pub amber: Color32,
    pub focus: Color32,
    pub scrim: Color32,
    pub shadow: Color32,
}

impl Palette {
    pub const LIGHT: Self = Self {
        is_dark: false,
        bg: Color32::from_rgb(0xef, 0xee, 0xe9),
        panel: Color32::from_rgb(0xf8, 0xf7, 0xf4),
        surface: Color32::from_rgb(0xe4, 0xe3, 0xde),
        surface_2: Color32::from_rgb(0xd9, 0xd8, 0xd2),
        line: Color32::from_rgb(0xd8, 0xd6, 0xcf),
        line_strong: Color32::from_rgb(0xbd, 0xbb, 0xb4),
        text: Color32::from_rgb(0x19, 0x19, 0x19),
        muted: Color32::from_rgb(0x69, 0x68, 0x63),
        faint: Color32::from_rgb(0xa3, 0xa1, 0x9a),
        lit: Color32::from_rgb(0x1e, 0x1e, 0x1e),
        on_lit: Color32::from_rgb(0xf4, 0xf3, 0xef),
        red: Color32::from_rgb(0xe8, 0x26, 0x2d),
        red_text: Color32::from_rgb(0xc2, 0x16, 0x1c),
        amber: Color32::from_rgb(0xa8, 0x64, 0x00),
        focus: Color32::from_rgb(0x19, 0x19, 0x19),
        scrim: Color32::from_rgba_premultiplied(7, 7, 7, 89), // rgb(20 20 20 / 0.35)
        shadow: Color32::from_rgba_premultiplied(0, 0, 0, 64),
    };

    pub const DARK: Self = Self {
        is_dark: true,
        bg: Color32::from_rgb(0x14, 0x14, 0x14),
        panel: Color32::from_rgb(0x1b, 0x1b, 0x1b),
        surface: Color32::from_rgb(0x22, 0x22, 0x22),
        surface_2: Color32::from_rgb(0x2b, 0x2b, 0x2b),
        line: Color32::from_rgb(0x2b, 0x2b, 0x2b),
        line_strong: Color32::from_rgb(0x3d, 0x3d, 0x3d),
        text: Color32::from_rgb(0xf2, 0xf1, 0xee),
        muted: Color32::from_rgb(0x8f, 0x8e, 0x89),
        faint: Color32::from_rgb(0x5c, 0x5b, 0x57),
        lit: Color32::from_rgb(0xf2, 0xf1, 0xee),
        on_lit: Color32::from_rgb(0x14, 0x14, 0x14),
        red: Color32::from_rgb(0xe8, 0x26, 0x2d),
        red_text: Color32::from_rgb(0xff, 0x4d, 0x53),
        amber: Color32::from_rgb(0xf2, 0xa9, 0x3b),
        focus: Color32::from_rgb(0xf2, 0xf1, 0xee),
        scrim: Color32::from_rgba_premultiplied(0, 0, 0, 153), // rgb(0 0 0 / 0.6)
        shadow: Color32::from_rgba_premultiplied(0, 0, 0, 160),
    };

    pub fn resolve(theme: Theme, ctx: &egui::Context) -> Self {
        match theme {
            Theme::Light => Self::LIGHT,
            Theme::Dark => Self::DARK,
            Theme::System => match ctx.system_theme() {
                Some(egui::Theme::Light) => Self::LIGHT,
                _ => Self::DARK,
            },
        }
    }
}

pub const RADIUS_PAD: u8 = 16;
pub const RADIUS_FIELD: u8 = 10;
pub const RADIUS_PANEL: u8 = 20;

pub fn font_body(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn font_medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(Arc::from(FAMILY_MEDIUM)))
}

pub fn font_heading(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(Arc::from(FAMILY_HEADING)))
}

pub fn font_mono(size: f32) -> FontId {
    FontId::monospace(size)
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "archivo-regular".into(),
        Arc::new(FontData::from_static(ARCHIVO_REGULAR)),
    );
    fonts.font_data.insert(
        "archivo-medium".into(),
        Arc::new(FontData::from_static(ARCHIVO_MEDIUM)),
    );
    fonts.font_data.insert(
        "archivo-heading".into(),
        Arc::new(FontData::from_static(ARCHIVO_HEADING)),
    );

    if let Some(prop) = fonts.families.get_mut(&FontFamily::Proportional) {
        prop.insert(0, "archivo-regular".into());
    }

    let mut fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fallback.retain(|n| n != "archivo-regular");

    let mut medium_list = vec!["archivo-medium".to_owned(), "archivo-regular".to_owned()];
    medium_list.extend(fallback.iter().cloned());
    fonts
        .families
        .insert(FontFamily::Name(Arc::from(FAMILY_MEDIUM)), medium_list);

    let mut heading_list = vec![
        "archivo-heading".to_owned(),
        "archivo-medium".to_owned(),
        "archivo-regular".to_owned(),
    ];
    heading_list.extend(fallback);
    fonts
        .families
        .insert(FontFamily::Name(Arc::from(FAMILY_HEADING)), heading_list);

    ctx.set_fonts(fonts);
}

pub fn apply_visuals(ctx: &egui::Context, p: &Palette) {
    let mut visuals = if p.is_dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    visuals.panel_fill = p.bg;
    visuals.window_fill = p.panel;
    visuals.extreme_bg_color = p.surface;
    visuals.faint_bg_color = p.surface;
    visuals.override_text_color = Some(p.text);
    visuals.selection.bg_fill = with_alpha(p.lit, if p.is_dark { 56 } else { 40 });
    visuals.selection.stroke = Stroke::new(1.5, p.focus);
    visuals.window_corner_radius = CornerRadius::same(RADIUS_PANEL);
    visuals.window_stroke = Stroke::new(1.0, p.line);
    visuals.menu_corner_radius = CornerRadius::same(14);
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: p.shadow,
    };
    visuals.window_shadow = egui::Shadow {
        offset: [0, 18],
        blur: 48,
        spread: 0,
        color: p.shadow,
    };

    visuals.widgets.noninteractive.bg_fill = p.surface;
    visuals.widgets.noninteractive.weak_bg_fill = p.surface;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(RADIUS_FIELD);

    visuals.widgets.inactive.bg_fill = p.surface;
    visuals.widgets.inactive.weak_bg_fill = p.surface;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(RADIUS_FIELD);

    visuals.widgets.hovered.bg_fill = p.surface_2;
    visuals.widgets.hovered.weak_bg_fill = p.surface_2;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.line_strong);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(RADIUS_FIELD);
    visuals.widgets.hovered.expansion = 0.0;

    visuals.widgets.active.bg_fill = p.surface_2;
    visuals.widgets.active.weak_bg_fill = p.surface_2;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, p.line_strong);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, p.text);
    visuals.widgets.active.corner_radius = CornerRadius::same(RADIUS_FIELD);
    visuals.widgets.active.expansion = 0.0;

    visuals.widgets.open.bg_fill = p.surface_2;
    visuals.widgets.open.weak_bg_fill = p.surface_2;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, p.line_strong);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, p.text);
    visuals.widgets.open.corner_radius = CornerRadius::same(RADIUS_FIELD);

    ctx.all_styles_mut(|style| {
        style.visuals = visuals.clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.scroll.bar_width = 7.0;
        style.spacing.scroll.floating = true;
        style
            .text_styles
            .insert(egui::TextStyle::Body, font_body(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, font_medium(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, font_body(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, font_heading(18.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, font_mono(12.5));
    });
}

/// Parses a `#rrggbb` hex colour string.
pub fn parse_hex(hex: &str) -> Color32 {
    let s = hex.trim().trim_start_matches('#');
    if s.len() == 6
        && let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&s[0..2], 16),
            u8::from_str_radix(&s[2..4], 16),
            u8::from_str_radix(&s[4..6], 16),
        )
    {
        return Color32::from_rgb(r, g, b);
    }
    Color32::from_rgb(0xe8, 0x26, 0x2d)
}

/// Linear sRGB blend matching CSS `color-mix(in srgb, a pct%, b)`.
pub fn mix(a: Color32, b: Color32, weight_a: f32) -> Color32 {
    let wa = weight_a.clamp(0.0, 1.0);
    let wb = 1.0 - wa;
    let r = (a.r() as f32 * wa + b.r() as f32 * wb).round() as u8;
    let g = (a.g() as f32 * wa + b.g() as f32 * wb).round() as u8;
    let bl = (a.b() as f32 * wa + b.b() as f32 * wb).round() as u8;
    let al = (a.a() as f32 * wa + b.a() as f32 * wb).round() as u8;
    Color32::from_rgba_unmultiplied(r, g, bl, al)
}

pub fn with_alpha(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}
