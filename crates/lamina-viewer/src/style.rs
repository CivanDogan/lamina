//! Palette, fonts and widget styling.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    Visuals,
};
use std::sync::Arc;

pub const CANVAS_EDGE: Color32 = Color32::from_rgb(0x0B, 0x0D, 0x10);
pub const PANEL: Color32 = Color32::from_rgb(0x10, 0x13, 0x18);
pub const RAISED: Color32 = Color32::from_rgb(0x16, 0x1A, 0x20);
pub const HOVER: Color32 = Color32::from_rgb(0x1C, 0x21, 0x29);
pub const HAIRLINE: Color32 = Color32::from_rgb(0x22, 0x28, 0x31);
pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEB);
pub const TEXT_2: Color32 = Color32::from_rgb(0x9A, 0xA3, 0xAE);
pub const TEXT_3: Color32 = Color32::from_rgb(0x6B, 0x74, 0x80);
pub const ACCENT: Color32 = Color32::from_rgb(0xE0, 0x8A, 0x3C);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x3A, 0x2A, 0x1C);
pub const ERROR: Color32 = Color32::from_rgb(0xFF, 0x5C, 0x7A);
pub const WARN: Color32 = Color32::from_rgb(0xF2, 0xB8, 0x4B);
pub const OK: Color32 = Color32::from_rgb(0x4C, 0xC3, 0x8A);

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn body(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

fn load(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let ui_font = load(&[
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/System/Library/Fonts/SFNS.ttf",
        "/usr/share/fonts/truetype/inter/Inter-Regular.ttf",
    ]);
    let ui_bold = load(&[
        "C:\\Windows\\Fonts\\seguisb.ttf",
        "/System/Library/Fonts/SFNS.ttf",
        "/usr/share/fonts/truetype/inter/Inter-SemiBold.ttf",
    ]);
    let mono_font = load(&[
        "C:\\Windows\\Fonts\\CascadiaMono.ttf",
        "/System/Library/Fonts/SFNSMono.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    ]);
    let mut semibold_family = Vec::new();
    if let Some(f) = ui_font {
        fonts
            .font_data
            .insert("ui".into(), Arc::new(FontData::from_owned(f)));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "ui".into());
    }
    if let Some(f) = ui_bold {
        fonts
            .font_data
            .insert("ui-semibold".into(), Arc::new(FontData::from_owned(f)));
        semibold_family.push("ui-semibold".to_string());
    }
    if let Some(f) = mono_font {
        fonts
            .font_data
            .insert("mono".into(), Arc::new(FontData::from_owned(f)));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "mono".into());
    }
    // Fall back to the proportional stack for glyphs the semibold face lacks.
    let prop = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    semibold_family.extend(prop);
    fonts
        .families
        .insert(FontFamily::Name("semibold".into()), semibold_family);
    ctx.set_fonts(fonts);

    let mut v = Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = RAISED;
    v.extreme_bg_color = CANVAS_EDGE;
    v.faint_bg_color = RAISED;
    v.code_bg_color = RAISED;
    v.override_text_color = None;
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = WARN;
    v.error_fg_color = ERROR;
    v.window_stroke = Stroke::new(1.0, HAIRLINE);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    let r = CornerRadius::same(6);
    for (w, bg, fg) in [
        (&mut v.widgets.noninteractive, PANEL, TEXT_2),
        (&mut v.widgets.inactive, RAISED, TEXT),
        (&mut v.widgets.hovered, HOVER, TEXT),
        (
            &mut v.widgets.active,
            Color32::from_rgb(0x24, 0x2A, 0x33),
            TEXT,
        ),
        (&mut v.widgets.open, HOVER, TEXT),
    ] {
        w.bg_fill = bg;
        w.weak_bg_fill = bg;
        w.corner_radius = r;
        w.fg_stroke = Stroke::new(1.0, fg);
        w.bg_stroke = Stroke::new(1.0, HAIRLINE);
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, HAIRLINE);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x30, 0x37, 0x42));
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    ctx.set_visuals(v);

    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.text_styles.insert(egui::TextStyle::Body, body(13.5));
        s.text_styles.insert(egui::TextStyle::Button, body(13.0));
        s.text_styles.insert(egui::TextStyle::Small, body(11.5));
        s.text_styles.insert(egui::TextStyle::Monospace, mono(12.5));
        s.text_styles
            .insert(egui::TextStyle::Heading, semibold(15.0));
    });
}

/// sRGBA from a scene theme colour.
pub fn rgba(c: lamina_scene::theme::Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}
