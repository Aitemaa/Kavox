//! Pure-vector icon renderer matching every icon in `src/components/ui.tsx`, plus `LogoMark` and `Wordmark`.

use std::f32::consts::PI;

use eframe::egui::{self, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, Vec2};

const BRAND_RED: Color32 = Color32::from_rgb(0xe8, 0x26, 0x2d);
const ICON_BG: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconName {
    Plus,
    Gear,
    Search,
    Mic,
    MicOff,
    Volume,
    Headphones,
    Broadcast,
    Stop,
    Play,
    Loop,
    Restart,
    Overlap,
    Toggle,
    Close,
    Trash,
    Check,
    Warning,
    Info,
    Folder,
    Copy,
    External,
    Dots,
    Pencil,
    Voice,
    Chipmunk,
    Pixie,
    Deep,
    Giant,
    Monster,
    Wobbly,
    Drunk,
    Alien,
}

/// Draws a 24x24 viewBox icon centered inside `rect` (or scaled to `size` at `center`).
pub fn paint_icon(painter: &Painter, center: Pos2, size: f32, name: IconName, color: Color32) {
    let origin = Pos2::new(center.x - size * 0.5, center.y - size * 0.5);
    let scale = size / 24.0;
    let stroke = Stroke::new((2.0 * scale).max(1.25), color);

    let pt = |x: f32, y: f32| Pos2::new(origin.x + x * scale, origin.y + y * scale);
    let circle = |cx: f32, cy: f32, r: f32, filled: bool| {
        if filled {
            painter.circle_filled(pt(cx, cy), r * scale, color);
        } else {
            painter.circle_stroke(pt(cx, cy), r * scale, stroke);
        }
    };
    let rect = |x: f32, y: f32, w: f32, h: f32, rx: f32, filled: bool| {
        let r = Rect::from_min_size(pt(x, y), Vec2::new(w * scale, h * scale));
        let cr = CornerRadius::same((rx * scale).round().clamp(0.0, 255.0) as u8);
        if filled {
            painter.rect_filled(r, cr, color);
        } else {
            painter.rect_stroke(r, cr, stroke, egui::StrokeKind::Middle);
        }
    };
    let path = |d: &str| draw_svg_path(painter, origin, scale, d, stroke, None);
    let filled_path = |d: &str| draw_svg_path(painter, origin, scale, d, Stroke::NONE, Some(color));

    match name {
        IconName::Plus => path("M12 5v14M5 12h14"),
        IconName::Gear => {
            circle(12.0, 12.0, 3.0, false);
            path(
                "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1.08-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1.08 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z",
            );
        }
        IconName::Search => {
            circle(11.0, 11.0, 7.0, false);
            path("m20 20-3.5-3.5");
        }
        IconName::Mic => {
            rect(9.0, 3.0, 6.0, 11.0, 3.0, false);
            path("M5 11a7 7 0 0 0 14 0M12 18v3");
        }
        IconName::MicOff => {
            path(
                "M15 9.34V6a3 3 0 0 0-5.94-.6M9 9v2a3 3 0 0 0 5.12 2.12M19 11a7 7 0 0 1-.78 3.22M5 11a7 7 0 0 0 11.5 5.36M12 18v3M3 3l18 18",
            );
        }
        IconName::Volume => {
            path("M11 5 6 9H2v6h4l5 4z");
            path("M19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.07");
        }
        IconName::Headphones => {
            path("M3 18v-6a9 9 0 0 1 18 0v6");
            path("M21 19a2 2 0 0 1-2 2h-1v-6h3zM3 19a2 2 0 0 0 2 2h1v-6H3z");
        }
        IconName::Broadcast => {
            path(
                "M4.9 19.1a10 10 0 0 1 0-14.2M19.1 4.9a10 10 0 0 1 0 14.2M7.8 16.2a6 6 0 0 1 0-8.4M16.2 7.8a6 6 0 0 1 0 8.4",
            );
            circle(12.0, 12.0, 2.0, false);
        }
        IconName::Stop => {
            rect(6.0, 6.0, 12.0, 12.0, 2.0, true);
        }
        IconName::Play => {
            filled_path("M8 5.5v13l10.5-6.5z");
        }
        IconName::Loop => {
            path("m17 2 4 4-4 4");
            path("M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4");
            path("M21 13v2a3 3 0 0 1-3 3H3");
        }
        IconName::Restart => {
            path("M3 12a9 9 0 1 0 3-6.7L3 8");
            path("M3 3v5h5");
        }
        IconName::Overlap => {
            path("m12 2 10 5-10 5L2 7z");
            path("m2 17 10 5 10-5M2 12l10 5 10-5");
        }
        IconName::Toggle => {
            path("M12 2v10");
            path("M18.4 6.6a9 9 0 1 1-12.8 0");
        }
        IconName::Close => path("M18 6 6 18M6 6l12 12"),
        IconName::Trash => path("M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14"),
        IconName::Check => path("M20 6 9 17l-5-5"),
        IconName::Warning => {
            path("M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z");
            path("M12 9v4");
            circle(12.0, 17.0, 0.9, true);
        }
        IconName::Info => {
            circle(12.0, 12.0, 10.0, false);
            path("M12 16v-4");
            circle(12.0, 8.0, 0.9, true);
        }
        IconName::Folder => {
            path("M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z");
        }
        IconName::Copy => {
            rect(9.0, 9.0, 13.0, 13.0, 2.0, false);
            path("M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1");
        }
        IconName::External => {
            path("M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6M15 3h6v6M10 14 21 3")
        }
        IconName::Dots => {
            circle(5.0, 12.0, 1.6, true);
            circle(12.0, 12.0, 1.6, true);
            circle(19.0, 12.0, 1.6, true);
        }
        IconName::Pencil => {
            path("M12 20h9");
            path("M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z");
        }
        IconName::Voice => path("M4 10v4M8 6v12M12 3v18M16 8v8M20 11v2"),
        IconName::Chipmunk => path("m6 11 6-6 6 6M6 18l6-6 6 6"),
        IconName::Pixie => {
            path("m11 3 1.9 5.1L18 10l-5.1 1.9L11 17l-1.9-5.1L4 10l5.1-1.9z");
            path("M19 16v5M16.5 18.5h5");
        }
        IconName::Deep => path("m7 9 5 5 5-5M7 19h10"),
        IconName::Giant => path("m6 6 6 6 6-6M6 13l6 6 6-6"),
        IconName::Monster => {
            path("M4 10c0-3.3 3.6-5.5 8-5.5s8 2.2 8 5.5v3.5l-2.5 3-2.5-3-3 3-3-3-2.5 3L4 13.5z");
        }
        IconName::Wobbly => {
            path("M3 12c1.7-6 3.3-6 5 0s3.3 6 5 0 3.3-6 5 0 1.7 3 3 3");
        }
        IconName::Drunk => {
            path("M8 4h8l-.9 6.5a3.1 3.1 0 0 1-6.2 0zM12 13.6V20M8.5 20h7");
        }
        IconName::Alien => {
            path("M12 3c4.8 0 8 3.8 8 8s-3.8 7.5-8 10c-4.2-2.5-8-5.8-8-10s3.2-8 8-8z");
            path("m8 10.5 2.5 1.5M16 10.5 13.5 12");
        }
    }
}

/// Draws the Kavox `LogoMark` (charcoal rounded tile + white K + red Morse dot).
pub fn draw_logo_mark(ui: &mut egui::Ui, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let scale = size / 32.0;
        let rx = (7.2 * scale).round().clamp(0.0, 255.0) as u8;
        p.rect_filled(rect, CornerRadius::same(rx), ICON_BG);

        let (lines, dot) = logo_k(9.85, 7.87, 16.27, 2.4);
        let stroke = Stroke::new((2.4 * scale).max(1.2), Color32::WHITE);
        let pt = |x: f32, y: f32| Pos2::new(rect.min.x + x * scale, rect.min.y + y * scale);
        for (x1, y1, x2, y2) in lines {
            draw_round_line(p, pt(x1, y1), pt(x2, y2), stroke);
        }
        p.circle_filled(pt(dot.0, dot.1), dot.2 * scale, BRAND_RED);
    }
    response
}

/// Draws the line-drawn "KAVOX" wordmark matching `Wordmark` in `src/components/ui.tsx`.
pub fn draw_wordmark(ui: &mut egui::Ui, height: f32, color: Color32) -> egui::Response {
    let width = height * 485.0 / 110.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let scale = height / 110.0;
        let pt = |x: f32, y: f32| {
            Pos2::new(
                rect.min.x + (x + 5.0) * scale,
                rect.min.y + (y + 5.0) * scale,
            )
        };
        let stroke = Stroke::new((10.0 * scale).max(1.2), color);

        // K
        let (lines, dot) = logo_k(0.0, 0.0, 100.0, 10.0);
        for (x1, y1, x2, y2) in lines {
            draw_round_line(p, pt(x1, y1), pt(x2, y2), stroke);
        }
        p.circle_filled(pt(dot.0, dot.1), dot.2 * scale, BRAND_RED);

        // A
        draw_round_polyline(
            p,
            &[pt(110.0, 100.0), pt(149.0, 0.0), pt(188.0, 100.0)],
            stroke,
        );
        // V
        draw_round_polyline(
            p,
            &[pt(188.0, 0.0), pt(227.0, 100.0), pt(266.0, 0.0)],
            stroke,
        );
        // O
        p.circle_stroke(pt(339.0, 50.0), 50.0 * scale, stroke);
        // X
        draw_round_line(p, pt(409.0, 0.0), pt(475.0, 100.0), stroke);
        draw_round_line(p, pt(475.0, 0.0), pt(409.0, 100.0), stroke);
    }
    response
}

type Segment = (f32, f32, f32, f32);
type Circle = (f32, f32, f32);

fn logo_k(x: f32, y: f32, h: f32, w: f32) -> ([Segment; 3], Circle) {
    let gap = 0.36 * w;
    let tip = x + 0.74 * h;
    let dot_x = x + w + gap;
    let dot_y = y + h * 0.5;
    let dot_r = w * 0.5;
    let arm = |ty: f32| {
        let dx = tip - dot_x;
        let dy = ty - dot_y;
        let n = dx.hypot(dy);
        let s = w + gap;
        (dot_x + (dx / n) * s, dot_y + (dy / n) * s, tip, ty)
    };
    (
        [(x, y, x, y + h), arm(y), arm(y + h)],
        (dot_x, dot_y, dot_r),
    )
}

fn draw_round_line(painter: &Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    painter.line_segment([a, b], stroke);
    let r = stroke.width * 0.5;
    painter.circle_filled(a, r, stroke.color);
    painter.circle_filled(b, r, stroke.color);
}

fn draw_round_polyline(painter: &Painter, pts: &[Pos2], stroke: Stroke) {
    if pts.len() < 2 {
        return;
    }
    painter.add(Shape::line(pts.to_vec(), stroke));
    let r = stroke.width * 0.5;
    for &p in pts {
        painter.circle_filled(p, r, stroke.color);
    }
}

// ---------- Lightweight SVG path tessellator (M/L/H/V/C/S/A/Z) ----------

fn draw_svg_path(
    painter: &Painter,
    origin: Pos2,
    scale: f32,
    d: &str,
    stroke: Stroke,
    fill: Option<Color32>,
) {
    let mut parser = PathParser::new(d);
    let mut cur = (0.0f32, 0.0f32);
    let mut start = (0.0f32, 0.0f32);
    let mut last_ctrl: Option<(f32, f32)> = None;
    let mut subpaths: Vec<(Vec<(f32, f32)>, bool)> = Vec::new();
    let mut current_pts: Vec<(f32, f32)> = Vec::new();
    let mut cmd = ' ';

    while let Some(next_cmd) = parser.next_command(cmd) {
        cmd = next_cmd;
        match cmd {
            'M' | 'm' => {
                if current_pts.len() >= 2 {
                    subpaths.push((std::mem::take(&mut current_pts), false));
                } else {
                    current_pts.clear();
                }
                let Some(x) = parser.next_f32() else { break };
                let Some(y) = parser.next_f32() else { break };
                cur = if cmd == 'm' {
                    (cur.0 + x, cur.1 + y)
                } else {
                    (x, y)
                };
                start = cur;
                current_pts.push(cur);
                last_ctrl = None;
                cmd = if cmd == 'm' { 'l' } else { 'L' };
            }
            'L' | 'l' => {
                let Some(x) = parser.next_f32() else { break };
                let Some(y) = parser.next_f32() else { break };
                cur = if cmd == 'l' {
                    (cur.0 + x, cur.1 + y)
                } else {
                    (x, y)
                };
                current_pts.push(cur);
                last_ctrl = None;
            }
            'H' | 'h' => {
                let Some(x) = parser.next_f32() else { break };
                cur.0 = if cmd == 'h' { cur.0 + x } else { x };
                current_pts.push(cur);
                last_ctrl = None;
            }
            'V' | 'v' => {
                let Some(y) = parser.next_f32() else { break };
                cur.1 = if cmd == 'v' { cur.1 + y } else { y };
                current_pts.push(cur);
                last_ctrl = None;
            }
            'C' | 'c' => {
                let Some(x1) = parser.next_f32() else { break };
                let Some(y1) = parser.next_f32() else { break };
                let Some(x2) = parser.next_f32() else { break };
                let Some(y2) = parser.next_f32() else { break };
                let Some(x) = parser.next_f32() else { break };
                let Some(y) = parser.next_f32() else { break };
                let (c1, c2, end) = if cmd == 'c' {
                    (
                        (cur.0 + x1, cur.1 + y1),
                        (cur.0 + x2, cur.1 + y2),
                        (cur.0 + x, cur.1 + y),
                    )
                } else {
                    ((x1, y1), (x2, y2), (x, y))
                };
                sample_cubic(cur, c1, c2, end, &mut current_pts);
                last_ctrl = Some(c2);
                cur = end;
            }
            'S' | 's' => {
                let Some(x2) = parser.next_f32() else { break };
                let Some(y2) = parser.next_f32() else { break };
                let Some(x) = parser.next_f32() else { break };
                let Some(y) = parser.next_f32() else { break };
                let c1 = match last_ctrl {
                    Some(prev) => (2.0 * cur.0 - prev.0, 2.0 * cur.1 - prev.1),
                    None => cur,
                };
                let (c2, end) = if cmd == 's' {
                    ((cur.0 + x2, cur.1 + y2), (cur.0 + x, cur.1 + y))
                } else {
                    ((x2, y2), (x, y))
                };
                sample_cubic(cur, c1, c2, end, &mut current_pts);
                last_ctrl = Some(c2);
                cur = end;
            }
            'A' | 'a' => {
                let Some(rx) = parser.next_f32() else { break };
                let Some(ry) = parser.next_f32() else { break };
                let Some(x_rot) = parser.next_f32() else {
                    break;
                };
                let Some(large_arc) = parser.next_flag() else {
                    break;
                };
                let Some(sweep) = parser.next_flag() else {
                    break;
                };
                let Some(x) = parser.next_f32() else { break };
                let Some(y) = parser.next_f32() else { break };
                let end = if cmd == 'a' {
                    (cur.0 + x, cur.1 + y)
                } else {
                    (x, y)
                };
                sample_arc(cur, rx, ry, x_rot, large_arc, sweep, end, &mut current_pts);
                cur = end;
                last_ctrl = None;
            }
            'Z' | 'z' => {
                if current_pts.len() >= 2 {
                    subpaths.push((std::mem::take(&mut current_pts), true));
                } else {
                    current_pts.clear();
                }
                cur = start;
                last_ctrl = None;
            }
            _ => break,
        }
    }
    if current_pts.len() >= 2 {
        subpaths.push((current_pts, false));
    }

    for (pts, closed) in subpaths {
        let screen: Vec<Pos2> = pts
            .into_iter()
            .map(|(x, y)| Pos2::new(origin.x + x * scale, origin.y + y * scale))
            .collect();
        if let Some(fill_color) = fill {
            painter.add(Shape::convex_polygon(
                screen.clone(),
                fill_color,
                Stroke::NONE,
            ));
        }
        if stroke.width > 0.0 && stroke.color != Color32::TRANSPARENT {
            if closed {
                painter.add(Shape::closed_line(screen, stroke));
            } else {
                let cap_r = stroke.width * 0.5;
                if let (Some(&first), Some(&last)) = (screen.first(), screen.last()) {
                    painter.circle_filled(first, cap_r, stroke.color);
                    painter.circle_filled(last, cap_r, stroke.color);
                }
                painter.add(Shape::line(screen, stroke));
            }
        }
    }
}

fn sample_cubic(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    out: &mut Vec<(f32, f32)>,
) {
    let steps = 12;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let u = 1.0 - t;
        let x =
            u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0;
        let y =
            u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1;
        out.push((x, y));
    }
}

#[allow(clippy::too_many_arguments)]
fn sample_arc(
    p0: (f32, f32),
    rx_in: f32,
    ry_in: f32,
    x_rot_deg: f32,
    large_arc: bool,
    sweep: bool,
    p1: (f32, f32),
    out: &mut Vec<(f32, f32)>,
) {
    let mut rx = rx_in.abs();
    let mut ry = ry_in.abs();
    if rx < 1e-5 || ry < 1e-5 || ((p0.0 - p1.0).abs() < 1e-5 && (p0.1 - p1.1).abs() < 1e-5) {
        out.push(p1);
        return;
    }

    let phi = x_rot_deg.to_radians();
    let (sin_p, cos_p) = phi.sin_cos();
    let dx2 = (p0.0 - p1.0) * 0.5;
    let dy2 = (p0.1 - p1.1) * 0.5;
    let x1p = cos_p * dx2 + sin_p * dy2;
    let y1p = -sin_p * dx2 + cos_p * dy2;

    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }

    let rx2 = rx * rx;
    let ry2 = ry * ry;
    let x1p2 = x1p * x1p;
    let y1p2 = y1p * y1p;
    let num = (rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2).max(0.0);
    let den = (rx2 * y1p2 + ry2 * x1p2).max(1e-9);
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let coef = sign * (num / den).sqrt();
    let cxp = coef * (rx * y1p / ry);
    let cyp = coef * -(ry * x1p / rx);

    let cx = cos_p * cxp - sin_p * cyp + (p0.0 + p1.0) * 0.5;
    let cy = sin_p * cxp + cos_p * cyp + (p0.1 + p1.1) * 0.5;

    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| -> f32 {
        let dot = ux * vx + uy * vy;
        let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        let cos = (dot / len.max(1e-9)).clamp(-1.0, 1.0);
        let a = cos.acos();
        if ux * vy - uy * vx < 0.0 { -a } else { a }
    };

    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;

    let theta1 = angle(1.0, 0.0, ux, uy);
    let mut dtheta = angle(ux, uy, vx, vy);
    if !sweep && dtheta > 0.0 {
        dtheta -= 2.0 * PI;
    } else if sweep && dtheta < 0.0 {
        dtheta += 2.0 * PI;
    }

    let steps = ((dtheta.abs() / (PI / 10.0)).ceil() as usize).clamp(4, 32);
    for i in 1..=steps {
        let t = theta1 + dtheta * (i as f32 / steps as f32);
        let (st, ct) = t.sin_cos();
        let x = cos_p * rx * ct - sin_p * ry * st + cx;
        let y = sin_p * rx * ct + cos_p * ry * st + cy;
        out.push((x, y));
    }
}

struct PathParser<'a> {
    bytes: &'a [u8],
    i: usize,
}

impl<'a> PathParser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            i: 0,
        }
    }

    fn skip_sep(&mut self) {
        while self.i < self.bytes.len()
            && matches!(self.bytes[self.i], b' ' | b'\t' | b'\n' | b'\r' | b',')
        {
            self.i += 1;
        }
    }

    fn next_command(&mut self, prev: char) -> Option<char> {
        self.skip_sep();
        if self.i >= self.bytes.len() {
            return None;
        }
        let c = self.bytes[self.i] as char;
        if c.is_ascii_alphabetic() {
            self.i += 1;
            Some(c)
        } else if prev != ' ' && prev != 'Z' && prev != 'z' {
            Some(prev)
        } else {
            None
        }
    }

    fn next_flag(&mut self) -> Option<bool> {
        self.skip_sep();
        if self.i >= self.bytes.len() {
            return None;
        }
        match self.bytes[self.i] {
            b'0' => {
                self.i += 1;
                Some(false)
            }
            b'1' => {
                self.i += 1;
                Some(true)
            }
            _ => None,
        }
    }

    fn next_f32(&mut self) -> Option<f32> {
        self.skip_sep();
        let start = self.i;
        if self.i < self.bytes.len() && matches!(self.bytes[self.i], b'+' | b'-') {
            self.i += 1;
        }
        let mut seen_dot = false;
        while self.i < self.bytes.len() {
            let b = self.bytes[self.i];
            if b.is_ascii_digit() {
                self.i += 1;
            } else if b == b'.' && !seen_dot {
                seen_dot = true;
                self.i += 1;
            } else {
                break;
            }
        }
        if self.i == start {
            return None;
        }
        std::str::from_utf8(&self.bytes[start..self.i])
            .ok()?
            .parse()
            .ok()
    }
}
