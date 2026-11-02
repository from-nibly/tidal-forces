use super::*;
use crate::lyrics::Lyrics;

#[cfg(test)]
#[path = "now_playing_tests.rs"]
mod tests;

#[derive(Default)]
pub(super) enum State {
    #[default]
    Idle,
    Loading,
    Ready(Lyrics),
    Missing,
    Failed(String),
}

#[derive(Default)]
pub(super) struct NowPlaying {
    pub open: bool,
    key: Option<(u64, u64)>,
    serial: u64,
    state: State,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    occurrence: Option<u64>,
    follow: bool,
    active: Option<usize>,
    focus: Option<egui::Id>,
    font_support: Option<bool>,
}

impl NowPlaying {
    pub fn close(&mut self) {
        self.open = false;
        self.cancel.take();
        if matches!(self.state, State::Loading) {
            self.state = State::Idle;
        }
    }
    pub fn reset(&mut self) {
        self.cancel.take();
        self.key = None;
        self.state = State::Idle;
        self.font_support = None;
        self.active = None;
        self.follow = true;
    }
}

impl App {
    #[cfg(any(test, debug_assertions))]
    pub(super) fn fixture_now_playing(&mut self, ctx: &egui::Context, kind: &str) {
        self.open_now_playing(ctx);
        let id = self.queue.current().unwrap().id;
        self.now_playing.key = self.account.map(|user| (user, id));
        self.now_playing.occurrence = self.queue.current_entry().map(|entry| entry.occurrence);
        self.position = 84;
        self.position_clock.observe(
            self.play_generation,
            std::time::Duration::from_secs(84),
            std::time::Instant::now(),
        );
        let words = [
            "A quiet morning opens",
            "The first light finds the water",
            "A line across the window",
            "Another page to turn",
            "We leave the noise behind us",
            "And take the longer way",
            "A moment held in color",
            "A place to pause and listen",
            "The next light is already here",
            "The words return to silence",
            "A small and steady rhythm",
            "The story carries on",
        ];
        let subtitles = if kind == "now-playing" {
            words
                .iter()
                .enumerate()
                .map(|(index, text)| {
                    format!("[{:02}:{:02}.000]{text}", index * 10 / 60, index * 10 % 60)
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };
        let text = if kind == "lyrics-rtl" {
            "שורת דוגמה ראשונה\nשורת דוגמה נוספת".to_owned()
        } else {
            words.join("\n")
        };
        self.now_playing.state = if kind == "lyrics-missing" {
            State::Missing
        } else {
            State::Ready(Lyrics::parse(id, &serde_json::json!({"trackId":id,"lyrics":text,"subtitles":subtitles,"lyricsProvider":"Synthetic layout fixture","isRightToLeft":kind == "lyrics-rtl"})).unwrap().unwrap())
        };
    }

    pub(super) fn open_now_playing(&mut self, ctx: &egui::Context) {
        if !self.now_playing.open {
            self.now_playing.focus = ctx.memory(|memory| memory.focused());
            self.now_playing.open = true;
            ctx.request_repaint();
            self.now_playing.follow = true;
            self.now_playing.active = None;
            if let Some(guard) = &mut self.restore_guard {
                guard.preserve_view = true;
            }
        }
    }

    pub(super) fn close_now_playing(&mut self, ctx: &egui::Context) {
        self.now_playing.close();
        if let Some(focus) = self.now_playing.focus.take() {
            ctx.memory_mut(|memory| memory.request_focus(focus));
        }
    }

    pub(super) fn lyrics_tick(&mut self) {
        let key = self.account.zip(self.queue.current().map(|track| track.id));
        if self.now_playing.key != key {
            self.now_playing.reset();
            self.now_playing.key = key;
        }
        let occurrence = self.queue.current_entry().map(|entry| entry.occurrence);
        if self.now_playing.occurrence != occurrence {
            self.now_playing.occurrence = occurrence;
            self.now_playing.follow = true;
            self.now_playing.active = None;
        }
        if !self.now_playing.open || !matches!(self.now_playing.state, State::Idle) {
            return;
        }
        let Some((user, id)) = key else {
            return;
        };
        self.now_playing.serial = self.now_playing.serial.wrapping_add(1);
        let (cancel, receiver) = tokio::sync::oneshot::channel();
        self.now_playing.cancel = Some(cancel);
        self.now_playing.state = State::Loading;
        if self
            .backend
            .tx
            .try_send(Request::Lyrics {
                user,
                serial: self.now_playing.serial,
                id,
                cancel: receiver,
            })
            .is_err()
        {
            self.now_playing.cancel.take();
            self.now_playing.state =
                State::Failed("The player is busy. Retry lyrics shortly.".into());
        }
    }

    pub(super) fn lyrics_result(
        &mut self,
        user: u64,
        serial: u64,
        id: u64,
        result: Result<Option<Lyrics>, String>,
    ) {
        if self.account != Some(user)
            || self.queue.current().map(|track| track.id) != Some(id)
            || self.now_playing.key != Some((user, id))
            || self.now_playing.serial != serial
            || !matches!(self.now_playing.state, State::Loading)
        {
            return;
        }
        self.now_playing.cancel.take();
        self.now_playing.font_support = None;
        self.now_playing.state = match result {
            Ok(Some(lyrics)) => State::Ready(lyrics),
            Ok(None) => State::Missing,
            Err(error) => State::Failed(error),
        };
    }

    pub(super) fn now_playing_view(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut radio = None;
        let mut artist = None;
        let mut album = None;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme_bg()).inner_margin(24)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("NOW PLAYING")).size(12.).color(theme_accent()).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button(tr("Back · Esc")).clicked();
                });
            });
            ui.add_space(16.);
            let Some(track) = self.queue.current().cloned() else {
                ui.heading(tr("Choose a track to get started"));
                ui.label(tr("Return to your library or search. Playback controls still use the same queue."));
                return;
            };
            let width = ui.available_width();
            let height = ui.available_height();
            let left = (width * 0.35).clamp(110., 420.);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(vec2(left, height), egui::Layout::top_down(egui::Align::Center), |ui| {
                    let cover = left.min((height - 130.).max(64.));
                    artwork(ui, track.cover_url(640), cover).context_menu(|ui| track_menu(ui, &track, &mut radio));
                    ui.add_space(14.);
                    ui.add(egui::Label::new(RichText::new(&track.title).size(22.).strong()).truncate()).on_hover_text(&track.title);
                    if ui.add(egui::Label::new(RichText::new(&track.artist.name).color(theme_accent())).truncate().sense(egui::Sense::click())).on_hover_text(tr("Open artist")).clicked() { artist = Some(track.artist.id); }
                    if ui.add(egui::Label::new(RichText::new(&track.album.title).color(theme_muted())).truncate().sense(egui::Sense::click())).on_hover_text(tr("Open album")).clicked() { album = Some((track.album.id, track.album.title.clone())); }
                });
                ui.add_space(16.);
                ui.allocate_ui_with_layout(vec2((width - left - 24.).max(120.), height), egui::Layout::top_down(egui::Align::Min), |ui| self.lyrics_panel(ui));
            });
        });
        if close {
            self.close_now_playing(ctx);
        }
        if let Some(id) = artist {
            self.navigate(Page::Artist(id));
        }
        if let Some((id, title)) = album {
            self.navigate(Page::Collection {
                kind: "albums".into(),
                id: id.to_string(),
                title,
            });
        }
        if let Some(action) = radio {
            self.apply_track_action(action);
        }
    }

    fn lyrics_panel(&mut self, ui: &mut egui::Ui) {
        let position = self.position_time();
        let confirmed = self.position_clock.is_confirmed(self.play_generation);
        let mut seek = None;
        let mut retry = false;
        match &self.now_playing.state {
            State::Idle | State::Loading => {
                ui.heading(tr("Lyrics"));
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr("Checking TIDAL lyrics…"));
                });
            }
            State::Missing => {
                ui.heading(tr("No lyrics available"));
                ui.label(tr(
                    "TIDAL has no lyrics available for this track. Your music keeps playing.",
                ));
                retry = ui.button(tr("Retry lyrics")).clicked();
            }
            State::Failed(message) => {
                ui.heading(tr("Lyrics unavailable"));
                ui.label(message);
                retry = ui.button(tr("Retry lyrics")).clicked();
            }
            State::Ready(lyrics) => {
                let supported = *self.now_playing.font_support.get_or_insert_with(|| {
                    !lyrics.right_to_left
                        && ui.fonts_mut(|fonts| {
                            let font = egui::FontId::proportional(18.);
                            lyrics
                                .plain
                                .chars()
                                .chain(lyrics.lines.iter().flat_map(|line| line.text.chars()))
                                .filter(|character| !character.is_whitespace())
                                .all(|character| fonts.has_glyph(&font, character))
                        })
                });
                if !supported {
                    ui.heading(tr("Lyrics display unavailable"));
                    ui.label(if lyrics.right_to_left {
                        "TIDAL supplied right-to-left lyrics. This renderer cannot yet display that writing direction correctly."
                    } else {
                        "These lyrics contain characters unsupported by the bundled fonts."
                    });
                    ui.label(tr("Playback and Now Playing remain available. No other lyrics service was contacted."));
                    ui.add_space(12.);
                    ui.label(
                        RichText::new(format!("Lyrics via TIDAL · {}", lyrics.provider))
                            .size(11.)
                            .color(theme_muted()),
                    );
                    return;
                }
                let synced = !lyrics.lines.is_empty();
                let active = confirmed.then(|| lyrics.active(position)).flatten();
                ui.horizontal(|ui| {
                    ui.heading(tr("Lyrics"));
                    if synced {
                        if ui
                            .selectable_label(self.now_playing.follow, tr("Follow"))
                            .on_hover_text(tr(
                                "Follow the confirmed playback position; scrolling pauses follow",
                            ))
                            .clicked()
                        {
                            self.now_playing.follow = true;
                            self.now_playing.active = None;
                        }
                    } else {
                        ui.label(
                            RichText::new(tr("Plain text · no timing"))
                                .color(theme_muted())
                                .size(12.),
                        );
                    }
                });
                let font_size = if ui.available_width() < 380. {
                    18.
                } else {
                    22.
                };
                let row_height = font_size * 2.6;
                let height = (ui.available_height() - 44.).max(60.);
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt(("lyrics", self.now_playing.key, self.now_playing.occurrence))
                    .max_height(height)
                    .auto_shrink([false, false]);
                if self.now_playing.follow
                    && let Some(active) = active
                {
                    scroll = scroll.vertical_scroll_offset(
                        (active as f32 * (row_height + ui.spacing().item_spacing.y) - height * 0.4)
                            .max(0.),
                    );
                }
                self.now_playing.active = active;
                let plain: Vec<_> = if synced {
                    Vec::new()
                } else {
                    lyrics.plain.lines().collect()
                };
                let count = if synced {
                    lyrics.lines.len()
                } else {
                    plain.len()
                };
                let output = scroll.show_rows(ui, row_height, count, |ui, range| {
                    for index in range {
                        let text = if synced {
                            lyrics.lines[index].text.as_str()
                        } else {
                            plain[index]
                        };
                        let highlighted = synced
                            && active.is_some_and(|active| {
                                lyrics.lines[index].at == lyrics.lines[active].at
                            });
                        let color = if highlighted {
                            theme_accent()
                        } else {
                            theme_muted()
                        };
                        let mut job = egui::text::LayoutJob::simple(
                            text.to_owned(),
                            egui::FontId::proportional(font_size),
                            color,
                            ui.available_width(),
                        );
                        job.wrap.max_rows = 2;
                        job.halign = if lyrics.right_to_left {
                            egui::Align::Max
                        } else {
                            egui::Align::Min
                        };
                        let response = ui.add_sized(
                            vec2(ui.available_width(), row_height),
                            egui::Label::new(job).sense(if synced {
                                egui::Sense::click()
                            } else {
                                egui::Sense::hover()
                            }),
                        );
                        if synced && response.clicked() {
                            seek = Some(lyrics.lines[index].at);
                        }
                    }
                });
                let manual = ui.input(|input| {
                    let over = input
                        .pointer
                        .hover_pos()
                        .is_some_and(|point| output.inner_rect.expand(12.).contains(point));
                    over && (input.raw_scroll_delta.y != 0.
                        || (input.pointer.any_down() && input.pointer.delta().y != 0.)
                        || input.key_pressed(egui::Key::PageDown)
                        || input.key_pressed(egui::Key::PageUp))
                });
                if manual {
                    self.now_playing.follow = false;
                }
                ui.add_space(8.);
                ui.label(
                    RichText::new(format!("Lyrics via TIDAL · {}", lyrics.provider))
                        .size(11.)
                        .color(theme_muted()),
                );
                if lyrics.right_to_left {
                    ui.label(RichText::new(tr("Right-to-left text retained; complex-script font/shaping support is not yet verified.")).size(10.).color(theme_muted()));
                }
            }
        }
        if retry {
            self.now_playing.state = State::Idle;
            self.now_playing.active = None;
            ui.ctx().request_repaint();
        }
        if let Some(position) = seek {
            self.dispatch(crate::actions::Action::Seek(position), ui.ctx());
            self.now_playing.follow = true;
            self.now_playing.active = None;
        }
    }
}
