use super::*;

impl App {
    pub(super) fn library_tabs(&mut self, ui: &mut egui::Ui) {
        let mut selected = None;
        ui.horizontal_wrapped(|ui| {
            for kind in FavoriteKind::ALL {
                if ui
                    .selectable_label(self.page == Page::Library(kind), kind.label())
                    .clicked()
                {
                    selected = Some(Page::Library(kind));
                }
            }
            if ui
                .selectable_label(self.page == Page::Playlists, "Playlists & folders")
                .clicked()
            {
                selected = Some(Page::Playlists);
            }
        });
        if let Some(page) = selected {
            self.navigate(page);
        }
        ui.add_space(if ui.ctx().content_rect().height() < 600. {
            8.
        } else {
            16.
        });
    }

    pub(super) fn set_favorite(&mut self, item: Favorite, saved: bool) {
        let Some(user) = self.account else {
            return;
        };
        if self.favorites.begin(item) && !self.try_send(Request::SetFavorite { user, item, saved })
        {
            self.favorites.finish(item, None);
        }
    }
}

pub(super) fn favorite_button(
    ui: &mut egui::Ui,
    favorites: &Favorites,
    item: Favorite,
    action: &mut Option<(Favorite, bool)>,
) {
    let state = favorites.state(item);
    ui.push_id(item, |ui| {
        ui.add_enabled_ui(item.id != 0 && !favorites.pending(item), |ui| {
            let response = icon_button(
                ui,
                Icon::Heart,
                match state {
                    Some(true) => "Remove from TIDAL favorites",
                    Some(false) => "Save to TIDAL favorites",
                    None => "Favorite actions · Saved status is not known yet · Opens a menu",
                },
                state == Some(true),
                28.,
            );
            if state.is_none() {
                paint_icon(
                    ui.painter(),
                    Icon::Down,
                    response.rect.right_top() + vec2(-4., 5.),
                    10.,
                    MUTED,
                );
            }
            if let Some(saved) = state {
                if response.clicked() {
                    *action = Some((item, !saved));
                }
            } else {
                egui::Popup::menu(&response).show(|ui| {
                    ui.label(
                        RichText::new("Favorite status not loaded")
                            .size(12.)
                            .color(MUTED),
                    );
                    if ui.button("Save to TIDAL favorites").clicked() {
                        *action = Some((item, true));
                        ui.close();
                    }
                    if ui.button("Remove from TIDAL favorites").clicked() {
                        *action = Some((item, false));
                        ui.close();
                    }
                });
            }
        });
    });
}
