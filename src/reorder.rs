use crate::model::PlaylistPage;
use anyhow::{Result, ensure};

pub const MAX_TRACKS: usize = 100;

#[derive(Clone)]
pub struct Move {
    pub user: u64,
    pub id: String,
    pub etag: String,
    pub ids: Vec<u64>,
    pub from: usize,
    pub to_end: bool,
}
impl Move {
    pub fn after(&self) -> Result<Vec<u64>> {
        ensure!(
            self.user != 0 && !self.etag.trim().is_empty(),
            "Missing account or playlist revision"
        );
        ensure!(
            (2..=MAX_TRACKS).contains(&self.ids.len()) && self.ids.iter().all(|id| *id != 0),
            "Reordering requires 2–100 available tracks"
        );
        ensure!(
            self.from < self.ids.len(),
            "The selected playlist position changed"
        );
        ensure!(
            self.from != if self.to_end { self.ids.len() - 1 } else { 0 },
            "The track is already at that end"
        );
        let mut ids = self.ids.clone();
        let id = ids.remove(self.from);
        if self.to_end {
            ids.push(id)
        } else {
            ids.insert(0, id)
        }
        Ok(ids)
    }
    pub fn matches(&self, page: &PlaylistPage, ids: &[u64]) -> bool {
        page.playlist.uuid == self.id
            && page.editable
            && page.playlist.number_of_videos == 0
            && page.playlist.number_of_tracks == ids.len() as u64
            && !page.more
            && page.next_offset == ids.len()
            && page.rows.len() == ids.len()
            && page
                .rows
                .iter()
                .enumerate()
                .all(|(position, (raw, track))| *raw == position && track.id == ids[position])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moves_choose_duplicate_occurrences_by_position() {
        let mut change = Move {
            user: 7,
            id: "list".into(),
            etag: "revision".into(),
            ids: vec![9, 8, 9],
            from: 2,
            to_end: false,
        };
        assert_eq!(change.after().unwrap(), [9, 9, 8]);
        change.from = 0;
        change.to_end = true;
        assert_eq!(change.after().unwrap(), [8, 9, 9]);
        change.from = 3;
        assert!(change.after().is_err());
        change.from = 0;
        change.ids.push(0);
        assert!(change.after().is_err());
        change.ids = vec![9; 101];
        assert!(change.after().is_err());
    }
}
