use super::*;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Sort {
    #[default]
    Original,
    Name,
    Artist,
}
impl Sort {
    fn label(self) -> &'static str {
        match self {
            Self::Original => "Original order",
            Self::Name => "Name",
            Self::Artist => "Artist",
        }
    }
}
#[derive(Default)]
pub(super) struct View {
    pub query: String,
    pub sort: Sort,
    pub reverse: bool,
    pub rows: Vec<usize>,
    source: Option<(Option<u64>, u64, FavoriteKind, usize)>,
    keys: Vec<(String, String)>,
    options: Option<(String, Sort, bool)>,
    dirty: bool,
}
impl View {
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    pub fn sync(
        &mut self,
        user: Option<u64>,
        generation: u64,
        kind: FavoriteKind,
        albums: &[Album],
        artists: &[Artist],
    ) -> bool {
        let len = if kind == FavoriteKind::Albums {
            albums.len()
        } else {
            artists.len()
        };
        let key = (user, generation, kind, len);
        if self
            .source
            .is_some_and(|old| (old.0, old.1, old.2) != (user, generation, kind))
        {
            self.query.clear();
            self.sort = Sort::Original;
            self.reverse = false;
        }
        let changed = self.dirty || self.source != Some(key);
        if changed {
            let append = !self.dirty
                && self.source.is_some_and(|old| {
                    (old.0, old.1, old.2) == (user, generation, kind) && old.3 <= len
                });
            if !append {
                self.keys.clear();
            }
            let start = self.keys.len();
            if kind == FavoriteKind::Albums {
                self.keys.extend(
                    albums[start..]
                        .iter()
                        .map(|a| (a.title.to_lowercase(), a.artist.name.to_lowercase())),
                );
            } else {
                self.keys.extend(
                    artists[start..]
                        .iter()
                        .map(|a| (a.name.to_lowercase(), String::new())),
                );
            }
            self.source = Some(key);
            self.dirty = false;
        }
        if !changed
            && self
                .options
                .as_ref()
                .is_some_and(|o| o.0 == self.query && o.1 == self.sort && o.2 == self.reverse)
        {
            return false;
        }
        let query = self.query.trim().to_lowercase();
        self.rows = self
            .keys
            .iter()
            .enumerate()
            .filter(|(_, (name, artist))| {
                query.is_empty() || name.contains(&query) || artist.contains(&query)
            })
            .map(|(i, _)| i)
            .collect();
        if self.sort != Sort::Original {
            self.rows.sort_by(|&a, &b| {
                let order = match self.sort {
                    Sort::Original => a.cmp(&b),
                    Sort::Name => self.keys[a].0.cmp(&self.keys[b].0),
                    Sort::Artist => self.keys[a]
                        .1
                        .cmp(&self.keys[b].1)
                        .then_with(|| self.keys[a].0.cmp(&self.keys[b].0)),
                };
                order.then_with(|| a.cmp(&b))
            });
        }
        if self.reverse {
            self.rows.reverse();
        }
        self.options = Some((self.query.clone(), self.sort, self.reverse));
        true
    }
}
impl App {
    pub(super) fn library_view_controls(&mut self, ui: &mut egui::Ui) {
        let Page::Library(kind @ (FavoriteKind::Albums | FavoriteKind::Artists)) = self.page else {
            return;
        };
        self.library_view.sync(
            self.account,
            self.generation,
            kind,
            &self.albums,
            &self.artists,
        );
        let before = (
            self.library_view.query.clone(),
            self.library_view.sort,
            self.library_view.reverse,
        );
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.library_view.query)
                    .id_salt("library-card-filter")
                    .hint_text(if kind == FavoriteKind::Albums {
                        "Filter loaded albums / artists"
                    } else {
                        "Filter loaded artists"
                    })
                    .desired_width(220.)
                    .char_limit(512),
            );
            egui::ComboBox::from_id_salt("library-card-sort")
                .selected_text(self.library_view.sort.label())
                .show_ui(ui, |ui| {
                    for sort in [Sort::Original, Sort::Name, Sort::Artist] {
                        if sort == Sort::Artist && kind != FavoriteKind::Albums {
                            continue;
                        }
                        ui.selectable_value(&mut self.library_view.sort, sort, sort.label());
                    }
                });
            let menu = ui
                .add(egui::Button::new("").min_size(vec2(34., 34.)))
                .on_hover_text("Library view actions");
            menu.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    "Library view actions",
                )
            });
            paint_icon(ui.painter(), Icon::Queue, menu.rect.center(), 16., MUTED);
            egui::Popup::menu(&menu).show(|ui| {
                ui.checkbox(&mut self.library_view.reverse, "Reverse view");
                if ui.button("Reset view").clicked() {
                    self.library_view.query.clear();
                    self.library_view.sort = Sort::Original;
                    self.library_view.reverse = false;
                    ui.close();
                }
            });
            self.library_load_controls(ui);
        });
        self.library_view.sync(
            self.account,
            self.generation,
            kind,
            &self.albums,
            &self.artists,
        );
        if before
            != (
                self.library_view.query.clone(),
                self.library_view.sort,
                self.library_view.reverse,
            )
        {
            self.browse_cache.pending_scroll = Some(0.);
        }
        let loaded = if kind == FavoriteKind::Albums {
            self.albums.len()
        } else {
            self.artists.len()
        };
        ui.label(RichText::new(format!("{} of {} loaded · view only",self.library_view.rows.len(),loaded)).size(12.).color(MUTED))
            .on_hover_text("Filtering and sorting apply only to loaded items. Load more or Load all to include the rest; raw pagination and saved order are unchanged.");
        if loaded > 0 && self.library_view.rows.is_empty() {
            ui.label("No loaded items match. Load more or reset the view.");
        }
        ui.add_space(8.);
    }
}

#[cfg(test)]
#[path = "library_view_tests.rs"]
mod tests;
