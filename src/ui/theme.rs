use eframe::egui::{self, Color32, Stroke, vec2};

pub(super) const BG: Color32 = Color32::from_rgb(12, 13, 15);
pub(super) const PANEL: Color32 = Color32::from_rgb(18, 19, 22);
pub(super) const CARD: Color32 = Color32::from_rgb(28, 30, 34);
pub(super) const BORDER: Color32 = Color32::from_rgb(43, 47, 53);
pub(super) const MUTED: Color32 = Color32::from_rgb(151, 155, 165);
pub(super) const ACCENT: Color32 = Color32::from_rgb(81, 225, 219);
pub(super) const PLAYING: Color32 = Color32::from_rgb(21, 37, 39);
pub(super) const TEXT: Color32 = Color32::from_rgb(241, 242, 245);

pub(super) fn configure(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = PANEL;
    visuals.faint_bg_color = CARD;
    visuals.selection.bg_fill = Color32::from_rgb(25, 67, 67);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    for (widget, fill) in [
        (&mut visuals.widgets.inactive, CARD),
        (&mut visuals.widgets.hovered, Color32::from_rgb(38, 44, 49)),
        (&mut visuals.widgets.active, PLAYING),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = Stroke::new(1.0_f32, BORDER);
        widget.corner_radius = egui::CornerRadius::same(6);
    }
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.slider_trailing_fill = true;
    visuals.widgets.noninteractive.fg_stroke.color = MUTED;
    visuals.override_text_color = Some(TEXT);
    ctx.set_visuals(visuals);
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
    ctx.style_mut(|style| {
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
