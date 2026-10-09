use super::*;

pub(super) fn page_title(ui: &mut egui::Ui, title: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(title)
                .size(if ui.ctx().content_rect().height() < 600. {
                    26.
                } else {
                    30.
                })
                .family(egui::FontFamily::Name("heading".into())),
        )
        .truncate(),
    )
    .on_hover_text(title);
}

pub(super) fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(
        RichText::new(title)
            .size(18.)
            .family(egui::FontFamily::Name("heading".into())),
    );
}

pub(super) fn hero(
    ui: &mut egui::Ui,
    cover: Option<String>,
    kind: &str,
    title: &str,
    subtitle: &str,
    framed: bool,
) {
    let width = ui.available_width();
    let size: f32 = if width < 460. || ui.ctx().content_rect().height() < 600. {
        64.
    } else {
        92.
    };
    egui::Frame::new()
        .fill(if framed { PLAYING } else { BG })
        .stroke(if framed {
            Stroke::new(1.0_f32, BORDER)
        } else {
            Stroke::NONE
        })
        .corner_radius(12)
        .inner_margin(if framed { 16 } else { 0 })
        .show(ui, |ui| {
            ui.set_width((width - if framed { 34. } else { 0. }).max(0.));
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), size.max(80.)),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    artwork(ui, cover, size);
                    ui.add_space(4.);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 6.;
                        ui.label(RichText::new(kind).size(10.).strong().color(ACCENT));
                        page_title(ui, title);
                        ui.add(
                            egui::Label::new(RichText::new(subtitle).size(12.).color(MUTED))
                                .truncate(),
                        )
                        .on_hover_text(subtitle);
                    });
                },
            );
        });
}

pub(super) fn action_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    primary: bool,
) -> egui::Response {
    let color = if primary { BG } else { TEXT };
    let galley = ui
        .painter()
        .layout_no_wrap(label.into(), egui::FontId::proportional(14.), color);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 48., 34.), egui::Sense::click());
    ui.painter().rect_filled(
        rect,
        6.,
        if primary {
            ACCENT
        } else if response.hovered() {
            PLAYING
        } else {
            CARD
        },
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.),
            8.,
            Stroke::new(1.0_f32, ACCENT),
            egui::StrokeKind::Outside,
        );
    }
    paint_icon(
        ui.painter(),
        icon,
        pos2(rect.left() + 20., rect.center().y),
        14.,
        color,
    );
    ui.painter().galley(
        pos2(rect.left() + 36., rect.center().y - galley.size().y / 2.),
        galley,
        color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn track_tile(
    ui: &mut egui::Ui,
    track: &Track,
    interactive: bool,
    playing: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), 44.),
        if interactive {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    if response.hovered() && interactive {
        ui.painter().rect_filled(rect, 6., CARD);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            6.,
            Stroke::new(1.0_f32, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(response.id)
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    content.spacing_mut().item_spacing.x = 8.;
    let cover = artwork(&mut content, track.cover_url(80), 40.);
    content.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.;
        ui.add(
            egui::Label::new(
                RichText::new(&track.title)
                    .size(13.)
                    .strong()
                    .color(if playing { ACCENT } else { TEXT }),
            )
            .truncate()
            .selectable(false),
        );
        ui.add(
            egui::Label::new(RichText::new(&track.artist.name).size(11.).color(MUTED))
                .truncate()
                .selectable(false),
        );
    });
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            interactive,
            format!("Play {} by {}", track.title, track.artist.name),
        )
    });
    if interactive {
        response
            .union(cover)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub(super) fn media_shelf<T>(
    ui: &mut egui::Ui,
    id: &str,
    items: &[T],
    grid: bool,
    footer: f32,
    mut draw: impl FnMut(&mut egui::Ui, &T, f32),
) {
    if items.is_empty() {
        return;
    }
    if grid {
        let columns = ((ui.available_width() + 12.) / 186.).floor().max(1.) as usize;
        let width =
            ((ui.available_width() - (columns - 1) as f32 * 12.) / columns as f32).min(220.);
        let stride = width + footer + 12.;
        let rows = items.len().div_ceil(columns);
        let (_, bounds) = ui.allocate_space(vec2(ui.available_width(), rows as f32 * stride - 12.));
        let start = ((ui.clip_rect().top() - bounds.top()) / stride)
            .floor()
            .max(0.) as usize;
        let end = (((ui.clip_rect().bottom() - bounds.top()) / stride)
            .ceil()
            .max(0.) as usize)
            .min(rows);
        for row in start.min(rows)..end {
            for col in 0..columns {
                let index = row * columns + col;
                if let Some(item) = items.get(index) {
                    let rect = egui::Rect::from_min_size(
                        bounds.min + vec2(col as f32 * (width + 12.), row as f32 * stride),
                        vec2(width, width + footer),
                    );
                    let mut cell = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt((id, index))
                            .max_rect(rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    draw(&mut cell, item, width);
                }
            }
        }
    } else {
        let width = 174.;
        let stride = width + 12.;
        egui::ScrollArea::horizontal()
            .id_salt(id)
            .show_viewport(ui, |ui, viewport| {
                let (_, bounds) =
                    ui.allocate_space(vec2(items.len() as f32 * stride - 12., width + footer));
                let start = ((viewport.left() / stride).floor().max(0.) as usize).min(items.len());
                let end = ((viewport.right() / stride).ceil().max(0.) as usize).min(items.len());
                for (index, item) in items.iter().enumerate().take(end).skip(start) {
                    let rect = egui::Rect::from_min_size(
                        bounds.min + vec2(index as f32 * stride, 0.),
                        vec2(width, width + footer),
                    );
                    let mut cell = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt((id, index))
                            .max_rect(rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    draw(&mut cell, item, width);
                }
            });
    }
}

pub(super) fn card(ui: &mut egui::Ui, width: f32, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(10)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width((width - 26.).max(0.));
                content(ui);
            });
        });
}
