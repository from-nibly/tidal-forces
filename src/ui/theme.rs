use eframe::egui::{self, Color32, Stroke, vec2};

use crate::palette::{Palette, Theme};
use std::cell::Cell;

thread_local! { static COLORS: Cell<Palette> = const { Cell::new(Palette::DARK) }; }

pub(super) struct PaletteScope(Palette);
impl Drop for PaletteScope {
    fn drop(&mut self) {
        COLORS.set(self.0);
    }
}
fn color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}
macro_rules! tokens {
    ($($name:ident => $field:ident),* $(,)?) => {$(
        pub(super) fn $name() -> Color32 { color(COLORS.get().$field) }
    )*};
}
tokens! {
    theme_bg => bg, theme_panel => panel, theme_card => card, theme_border => border,
    theme_muted => muted, theme_accent => accent, theme_playing => playing,
    theme_selected => selected, theme_text => text, theme_hover => hover, theme_error => error,
}
pub(super) fn theme_scrim() -> Color32 {
    let palette = COLORS.get();
    let [r, g, b] = palette.bg;
    Color32::from_rgba_unmultiplied(r, g, b, if palette.dark() { 80 } else { 230 })
}
pub(super) fn theme_on_accent() -> Color32 {
    color(COLORS.get().on_accent())
}

pub(super) fn scope(ctx: &egui::Context, mode: Theme, custom: Palette) -> PaletteScope {
    let palette = Palette::resolve(mode, ctx.system_theme() != Some(egui::Theme::Light), custom);
    let previous = COLORS.replace(palette);
    apply(ctx, palette);
    PaletteScope(previous)
}

fn apply(ctx: &egui::Context, palette: Palette) {
    ctx.set_theme(if palette.dark() {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let id = egui::Id::new("tidal_semantic_palette");
    if ctx.data(|data| data.get_temp::<Palette>(id)) == Some(palette) {
        return;
    }
    let mut visuals = if palette.dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = color(palette.bg);
    visuals.window_fill = color(palette.panel);
    visuals.extreme_bg_color = color(palette.panel);
    visuals.faint_bg_color = color(palette.card);
    visuals.selection.bg_fill = color(palette.selected);
    visuals.selection.stroke = Stroke::new(1.0_f32, color(palette.accent));
    for (widget, fill) in [
        (&mut visuals.widgets.inactive, palette.card),
        (&mut visuals.widgets.hovered, palette.hover),
        (&mut visuals.widgets.active, palette.playing),
    ] {
        widget.bg_fill = color(fill);
        widget.weak_bg_fill = color(fill);
        widget.bg_stroke = Stroke::new(1.0_f32, color(palette.border));
        widget.fg_stroke.color = color(palette.text);
        widget.corner_radius = egui::CornerRadius::same(6);
    }
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, color(palette.accent));
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.window_stroke = Stroke::new(1.0_f32, color(palette.border));
    visuals.slider_trailing_fill = true;
    visuals.widgets.noninteractive.fg_stroke.color = color(palette.muted);
    visuals.override_text_color = Some(color(palette.text));
    visuals.weak_text_color = Some(color(palette.muted));
    visuals.error_fg_color = color(palette.error);
    visuals.hyperlink_color = color(palette.accent);
    ctx.set_visuals(visuals);
    ctx.data_mut(|data| data.insert_temp(id, palette));
}

pub(super) fn configure(ctx: &egui::Context) {
    apply(ctx, Palette::DARK);
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/Inter-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "Inter Bold".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/Inter-Bold.ttf")).into(),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "Inter".into());
    fonts.families.insert(
        egui::FontFamily::Name("heading".into()),
        vec!["Inter Bold".into()],
    );
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = vec2(12., 8.);
        style.spacing.button_padding = vec2(10., 6.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.));
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(28., egui::FontFamily::Name("heading".into())),
        );
    });
}
