use super::*;

impl App {
    pub(super) fn library_tabs(&mut self, ui: &mut egui::Ui) {
        let mut selected = None;
        ui.scope(|ui| {
            ui.spacing_mut().button_padding = vec2(9., 6.);
            ui.spacing_mut().interact_size.y = 32.;
            ui.visuals_mut().selection.bg_fill = Color32::from_rgb(25, 52, 54);
            ui.horizontal_wrapped(|ui| {
                for kind in FavoriteKind::ALL {
                    if ui
                        .selectable_label(
                            self.page == Page::Library(kind),
                            RichText::new(kind.label()).size(12.).color(
                                if self.page == Page::Library(kind) {
                                    ACCENT
                                } else {
                                    MUTED
                                },
                            ),
                        )
                        .clicked()
                    {
                        selected = Some(Page::Library(kind));
                    }
                }
                if ui
                    .selectable_label(
                        self.page == Page::Playlists,
                        RichText::new("Playlists & folders").size(12.).color(
                            if self.page == Page::Playlists {
                                ACCENT
                            } else {
                                MUTED
                            },
                        ),
                    )
                    .clicked()
                {
                    selected = Some(Page::Playlists);
                }
            });
        });
        ui.add_space(8.);
        ui.separator();
        if let Some(page) = selected {
            self.navigate(page);
        }
        ui.add_space(12.);
    }

    pub(super) fn set_favorite(&mut self, item: Favorite, saved: bool) {
        let Some(user) = self.account else {
            return;
        };
        if self.favorites.begin(item) {
            self.stop_favorite_cache("Saved-status check stopped after a favorites change. Refresh to check the remaining items.");
            if !self.try_send(Request::SetFavorite { user, item, saved }) {
                self.favorites.finish(item, None);
            }
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
            // A background membership read must not turn an in-progress menu
            // click into an unrequested save/remove action.
            let press_id = response.id.with("membership-on-press");
            if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
                ui.data_mut(|data| data.insert_temp(press_id, state));
            }
            let action_state = if response.clicked() {
                let pressed = ui.data_mut(|data| data.remove_temp::<Option<bool>>(press_id));
                if ui.input(|i| i.pointer.primary_released()) {
                    pressed.unwrap_or(state)
                } else {
                    state
                }
            } else {
                if !ui.input(|i| i.pointer.primary_down()) {
                    ui.data_mut(|data| data.remove::<Option<bool>>(press_id));
                }
                state
            };
            let menu_open =
                egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
            if let Some(saved) = action_state.filter(|_| !menu_open) {
                if response.clicked() {
                    *action = Some((item, !saved));
                }
            } else {
                egui::Popup::menu(&response).show(|ui| {
                    ui.label(
                        RichText::new(if state.is_none() {
                            "Favorite status not loaded"
                        } else {
                            "Favorite actions"
                        })
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
