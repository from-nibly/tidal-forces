use super::*;
use crate::pins::Pins;

impl App {
    pub(super) fn load_pins(&mut self) {
        self.pins = None;
        self.pins_error = None;
        let Some(user) = self.account.filter(|user| *user != 0) else {
            return;
        };
        let result = self
            .pins_root
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No local settings directory"))
            .and_then(|root| Pins::load(root, user));
        match result {
            Ok(pins) => self.pins = Some(pins),
            Err(_) => {
                self.pins_error =
                    Some("Local pins could not be read. Existing files were left unchanged.".into())
            }
        }
    }
    fn save_pins(&mut self, next: Pins) {
        if !self.connected || self.account != Some(next.user) {
            return;
        }
        let Some(root) = &self.pins_root else { return };
        match next.save(root, next.user) {
            Ok(()) => self.pins = Some(next),
            Err(_) => {
                self.pins = None;
                self.pins_error = Some(
                    "Saving local pins could not be confirmed. Reload before trying again.".into(),
                );
            }
        }
    }
    pub(super) fn toggle_pin(&mut self, id: &str, title: &str) {
        let Some(mut next) = self
            .pins
            .clone()
            .filter(|pins| self.connected && self.account == Some(pins.user))
        else {
            return;
        };
        match next.toggle(id, title) {
            Ok(()) => self.save_pins(next),
            Err(error) => self.notice = Some(error.to_string()),
        }
    }
    fn move_pin(&mut self, id: &str, up: bool) {
        if let Some(mut next) = self
            .pins
            .clone()
            .filter(|pins| self.connected && self.account == Some(pins.user))
        {
            next.move_pin(id, up);
            self.save_pins(next);
        }
    }
    pub(super) fn pin_control(&mut self, ui: &mut egui::Ui) {
        let Some(page) = self.playlist_page.as_ref() else {
            return;
        };
        let id = page.playlist.uuid.clone();
        let title = page.playlist.title.clone();
        if !matches!(&self.page,Page::Collection{kind,id:current,..} if kind=="playlists" && current==&id)
        {
            return;
        }
        let pinned = self
            .pins
            .as_ref()
            .is_some_and(|pins| self.account == Some(pins.user) && pins.contains(&id));
        let enabled = self.connected
            && !self.loading
            && self.pins_root.is_some()
            && self
                .pins
                .as_ref()
                .is_some_and(|pins| self.account == Some(pins.user));
        if ui.add_enabled(enabled,egui::Button::new(if pinned {"Unpin locally"} else {"Pin locally"})).on_hover_text("A shortcut on this device, for this account only. Does not change TIDAL favorites or folder order. Up to 64 playlists.").clicked() {self.toggle_pin(&id,&title);}
    }
    pub(super) fn pinned_playlists(&mut self, ui: &mut egui::Ui) {
        if !self.connected {
            return;
        }
        if let Some(error) = self.pins_error.clone() {
            ui.label(
                RichText::new("Local pins unavailable")
                    .size(12.)
                    .color(MUTED),
            )
            .on_hover_text(error);
            if ui.small_button("Reload local pins").clicked() {
                self.load_pins();
            }
        }
        let Some(pins) = self
            .pins
            .as_ref()
            .filter(|pins| self.account == Some(pins.user))
        else {
            return;
        };
        let entries = pins.entries.clone();
        if entries.is_empty() {
            return;
        }
        ui.label(
            RichText::new("PINNED · THIS DEVICE")
                .size(10.)
                .color(MUTED)
                .strong(),
        );
        let count = entries.len();
        for (index, pin) in entries.into_iter().enumerate() {
            let fresh = self
                .playlist_page
                .as_ref()
                .map(|page| &page.playlist)
                .filter(|p| p.uuid == pin.id)
                .or_else(|| {
                    self.folders
                        .values()
                        .flat_map(|entries| entries.iter())
                        .find_map(|entry| match entry {
                            LibraryEntry::Playlist(p) if p.uuid == pin.id => Some(p),
                            _ => None,
                        })
                });
            let playlist = fresh.cloned().unwrap_or(Playlist {
                uuid: pin.id.clone(),
                title: pin.title.clone(),
                ..Default::default()
            });
            let active = matches!(&self.page,Page::Collection{kind,id,..} if kind=="playlists" && id==&pin.id);
            ui.push_id(("local-pin", &pin.id), |ui| {
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 28.).max(32.);
                    let response = ui
                        .allocate_ui_with_layout(
                            vec2(width, 44.),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| playlist_button(ui, &playlist, active),
                        )
                        .inner
                        .on_hover_text(format!("{} · Local shortcut", playlist.title));
                    if response.clicked() {
                        self.navigate(Page::Collection {
                            kind: "playlists".into(),
                            id: pin.id.clone(),
                            title: playlist.title.clone(),
                        });
                    }
                    let menu = ui.menu_button("⋯", |ui| {
                        if ui
                            .add_enabled(index > 0, egui::Button::new("Move pin up"))
                            .clicked()
                        {
                            self.move_pin(&pin.id, true);
                            ui.close();
                        }
                        if ui
                            .add_enabled(index + 1 < count, egui::Button::new("Move pin down"))
                            .clicked()
                        {
                            self.move_pin(&pin.id, false);
                            ui.close();
                        }
                        if ui.button("Unpin locally").clicked() {
                            self.toggle_pin(&pin.id, &pin.title);
                            ui.close();
                        }
                    });
                    menu.response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            "Pinned playlist actions",
                        )
                    });
                    menu.response.on_hover_text("Move or remove this local pin");
                });
            });
        }
        ui.separator();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::fixture;
    #[test]
    fn local_pin_changes_never_dispatch_account_requests_and_keep_account_scope() {
        let root = tempfile::tempdir().unwrap();
        let (_, mut app, _events, mut requests) = fixture();
        app.pins_root = Some(root.path().to_owned());
        app.load_pins();
        let revision = app.queue.revision();
        app.toggle_pin("one", "One");
        app.toggle_pin("two", "Two");
        app.move_pin("two", true);
        assert!(requests.try_recv().is_err());
        assert_eq!(app.queue.revision(), revision);
        assert_eq!(Pins::load(root.path(), 7).unwrap().entries[0].id, "two");
        app.account = Some(8);
        app.toggle_pin("cross-account", "No");
        assert!(!root.path().join("pins-8.json").exists());
        app.load_pins();
        assert!(app.pins.as_ref().unwrap().entries.is_empty());
        app.account = None;
        app.load_pins();
        assert!(app.pins.is_none());
    }
    fn draw(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(500., 600.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.pinned_playlists(ui));
            },
        )
    }
    fn label(output: &egui::FullOutput, value: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == value => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "Missing {value}; labels: {:?}",
                    output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) => Some(&text.galley.job.text),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                )
            })
    }
    fn click(ctx: &egui::Context, app: &mut App, pos: egui::Pos2) -> egui::FullOutput {
        let _ = draw(
            ctx,
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        draw(
            ctx,
            app,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        )
    }
    #[test]
    fn visible_pin_menu_reorders_without_navigation_or_remote_writes() {
        let root = tempfile::tempdir().unwrap();
        let (ctx, mut app, _events, mut requests) = fixture();
        app.pins_root = Some(root.path().into());
        app.load_pins();
        app.toggle_pin("one", "One");
        app.toggle_pin("two", "Two");
        let _ = draw(&ctx, &mut app, vec![]);
        let output = draw(&ctx, &mut app, vec![]);
        let _ = click(&ctx, &mut app, label(&output, "⋯"));
        let output = draw(&ctx, &mut app, vec![]);
        let _ = click(&ctx, &mut app, label(&output, "Move pin down"));
        assert_eq!(app.pins.as_ref().unwrap().entries[0].id, "two");
        assert!(requests.try_recv().is_err());
    }
    #[test]
    fn unreadable_pin_state_disables_writes_instead_of_erasing_it() {
        let root = tempfile::tempdir().unwrap();
        let (_, mut app, _events, _requests) = fixture();
        std::fs::write(root.path().join("pins-7.json"), b"invalid").unwrap();
        app.pins_root = Some(root.path().into());
        app.load_pins();
        app.toggle_pin("one", "One");
        assert!(app.pins.is_none());
        assert!(app.pins_error.is_some());
        assert_eq!(
            std::fs::read(root.path().join("pins-7.json")).unwrap(),
            b"invalid"
        );
    }
}
