use super::*;

pub(super) struct Drag {
    user: u64,
    generation: u64,
    id: String,
    etag: String,
    ids: Vec<u64>,
    from: usize,
}
impl App {
    pub(super) fn playlist_drag(&self, from: usize) -> Option<Drag> {
        if !self.can_reorder_playlist() || from >= self.tracks.len() {
            return None;
        }
        let page = self.playlist_page.as_ref()?;
        Some(Drag {
            user: self.account?,
            generation: self.generation,
            id: page.playlist.uuid.clone(),
            etag: page.etag.clone(),
            ids: self.tracks.iter().map(|t| t.id).collect(),
            from,
        })
    }
    pub(super) fn playlist_drag_current(&self, drag: &Drag) -> bool {
        self.can_reorder_playlist()
            && self.account == Some(drag.user)
            && self.generation == drag.generation
            && self.tracks.len() == drag.ids.len()
            && self
                .playlist_page
                .as_ref()
                .is_some_and(|p| p.playlist.uuid == drag.id && p.etag == drag.etag)
    }
    pub(super) fn playlist_drop_position(
        &self,
        drag: &Drag,
        row: usize,
        after: bool,
    ) -> Option<usize> {
        if !self.playlist_drag_current(drag) || row >= self.tracks.len() {
            return None;
        }
        let boundary = row + usize::from(after);
        let target = boundary - usize::from(boundary > drag.from);
        (target != drag.from).then_some(target)
    }
    pub(super) fn drop_playlist(&mut self, drag: &Drag, target: usize) {
        if !self.playlist_drag_current(drag)
            || !self
                .tracks
                .iter()
                .map(|t| t.id)
                .eq(drag.ids.iter().copied())
        {
            return;
        }
        self.open_reorder(drag.from, crate::reorder::Target::Position(target));
    }
    pub(super) fn playlist_drag_scroll(&self, ui: &egui::Ui) {
        let Some(drag) = egui::DragAndDrop::payload::<Drag>(ui.ctx()) else {
            return;
        };
        if !self.playlist_drag_current(&drag) {
            return;
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let clip = ui.clip_rect();
        if !clip.contains(pointer) {
            return;
        }
        let speed = if pointer.y < clip.top() + 32. {
            320.
        } else if pointer.y > clip.bottom() - 32. {
            -320.
        } else {
            0.
        };
        if speed != 0. {
            ui.scroll_with_delta(vec2(0., speed * ui.input(|i| i.stable_dt.min(0.05))));
            ui.ctx().request_repaint();
        }
    }
}
