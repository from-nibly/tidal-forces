use crate::model::PlaylistPage;
use anyhow::{Result, ensure};

pub const MAX_TRACKS: usize = crate::queue::MAX_ENTRIES;

#[derive(Clone)]
pub struct Check {
    pub user: u64,
    pub id: String,
    pub etag: String,
    pub count: usize,
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Top,
    Up,
    Down,
    Bottom,
}
impl Target {
    pub fn label(self) -> &'static str {
        match self {
            Self::Top => "to the top",
            Self::Up => "up one position",
            Self::Down => "down one position",
            Self::Bottom => "to the bottom",
        }
    }
}

#[derive(Clone)]
pub struct Move {
    pub user: u64,
    pub id: String,
    pub etag: String,
    pub ids: Vec<u64>,
    pub from: usize,
    pub target: Target,
}
impl Move {
    pub fn indices(&self) -> Result<(usize, usize)> {
        ensure!(
            self.user != 0 && !self.etag.trim().is_empty(),
            "Missing account or playlist revision"
        );
        ensure!(
            (2..=MAX_TRACKS).contains(&self.ids.len()) && self.ids.iter().all(|id| *id != 0),
            "Reordering requires 2–50,000 available tracks"
        );
        ensure!(
            self.from < self.ids.len(),
            "The selected playlist position changed"
        );
        ensure!(
            match self.target {
                Target::Top | Target::Up => self.from > 0,
                Target::Bottom | Target::Down => self.from + 1 < self.ids.len(),
            },
            "The track is already at that end"
        );
        Ok(match self.target {
            Target::Top => (self.from, 0),
            Target::Up => (self.from, self.from - 1),
            // Moving the next occurrence before this one avoids ambiguous forward-insertion offsets.
            Target::Down => (self.from + 1, self.from),
            Target::Bottom => (self.from, self.ids.len()),
        })
    }
    pub fn after(&self) -> Result<Vec<u64>> {
        let (from, to) = self.indices()?;
        let mut ids = self.ids.clone();
        let id = ids.remove(from);
        ids.insert(to.min(ids.len()), id);
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
    fn adjacent_moves_are_exact_swaps_and_never_use_forward_insertion() {
        for len in 2..=100 {
            for from in 0..len {
                for target in [Target::Up, Target::Down] {
                    let change = Move {
                        user: 7,
                        id: "list".into(),
                        etag: "revision".into(),
                        ids: (0..len).map(|i| (i % 3 + 1) as u64).collect(),
                        from,
                        target,
                    };
                    if (target == Target::Up && from == 0)
                        || (target == Target::Down && from + 1 == len)
                    {
                        assert!(change.after().is_err());
                        continue;
                    }
                    let mut expected = change.ids.clone();
                    expected.swap(
                        from,
                        if target == Target::Up {
                            from - 1
                        } else {
                            from + 1
                        },
                    );
                    assert_eq!(change.after().unwrap(), expected);
                    let (source, destination) = change.indices().unwrap();
                    assert_eq!(source, destination + 1);
                }
            }
        }
    }
    #[test]
    fn moves_choose_duplicate_occurrences_by_position() {
        let mut change = Move {
            user: 7,
            id: "list".into(),
            etag: "revision".into(),
            ids: vec![9, 8, 9],
            from: 2,
            target: Target::Top,
        };
        assert_eq!(change.after().unwrap(), [9, 9, 8]);
        change.from = 0;
        change.target = Target::Bottom;
        assert_eq!(change.after().unwrap(), [8, 9, 9]);
        change.from = 3;
        assert!(change.after().is_err());
        change.from = 0;
        change.ids.push(0);
        assert!(change.after().is_err());
        change.ids = vec![9; MAX_TRACKS + 1];
        assert!(change.after().is_err());
    }
}
