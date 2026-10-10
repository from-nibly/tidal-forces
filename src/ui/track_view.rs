use crate::model::Track;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Sort {
    #[default]
    Original,
    Title,
    Artist,
    Album,
    Duration,
}
impl Sort {
    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "Original order",
            Self::Title => "Title",
            Self::Artist => "Artist",
            Self::Album => "Album",
            Self::Duration => "Duration",
        }
    }
}

#[derive(Default)]
pub(super) struct View {
    pub query: String,
    pub sort: Sort,
    pub reverse: bool,
    pub rows: Vec<usize>,
    source: Option<(Option<u64>, u64, usize)>,
    keys: Vec<(String, String, String, u64)>,
    options: Option<(String, Sort, bool)>,
    dirty: bool,
}
impl View {
    pub fn active(&self) -> bool {
        !self.query.trim().is_empty() || self.sort != Sort::Original || self.reverse
    }
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    /// Only the projection is reordered; source positions keep occurrence identity.
    pub fn sync(&mut self, user: Option<u64>, generation: u64, tracks: &[Track]) -> bool {
        let key = (user, generation, tracks.len());
        if self
            .source
            .is_some_and(|old| (old.0, old.1) != (user, generation))
        {
            self.query.clear();
            self.sort = Sort::Original;
            self.reverse = false;
        }
        let changed = self.dirty || self.source != Some(key);
        if changed {
            self.keys = tracks
                .iter()
                .map(|t| {
                    (
                        t.title.to_lowercase(),
                        t.artist.name.to_lowercase(),
                        t.album.title.to_lowercase(),
                        t.duration,
                    )
                })
                .collect();
            self.source = Some(key);
            self.dirty = false;
        }
        if !changed
            && self.options.as_ref().is_some_and(|old| {
                old.0 == self.query && old.1 == self.sort && old.2 == self.reverse
            })
        {
            return false;
        }
        let query = self.query.trim().to_lowercase();
        self.rows = self
            .keys
            .iter()
            .enumerate()
            .filter(|(_, (title, artist, album, _))| {
                query.is_empty()
                    || title.contains(&query)
                    || artist.contains(&query)
                    || album.contains(&query)
            })
            .map(|(i, _)| i)
            .collect();
        self.rows.sort_by(|&a, &b| {
            let akey = &self.keys[a];
            let bkey = &self.keys[b];
            let order = match self.sort {
                Sort::Original => a.cmp(&b),
                Sort::Title => akey.0.cmp(&bkey.0),
                Sort::Artist => akey.1.cmp(&bkey.1),
                Sort::Album => akey.2.cmp(&bkey.2),
                Sort::Duration => akey.3.cmp(&bkey.3),
            };
            if self.reverse { order.reverse() } else { order }
        });
        self.options = Some((self.query.clone(), self.sort, self.reverse));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::track;
    #[test]
    fn projection_preserves_duplicate_occurrences_and_stable_ties() {
        let mut tracks = vec![track(9), track(8), track(9)];
        tracks[0].title = "Zulu".into();
        tracks[1].title = "Alpha".into();
        tracks[2].title = "Zulu".into();
        let mut view = View {
            sort: Sort::Title,
            ..Default::default()
        };
        assert!(view.sync(Some(7), 1, &tracks));
        assert_eq!(view.rows, [1, 0, 2]);
        assert!(!view.sync(Some(7), 1, &tracks));
        view.reverse = true;
        view.sync(Some(7), 1, &tracks);
        assert_eq!(view.rows, [0, 2, 1]);
        view.query = "zULu".into();
        view.sync(Some(7), 1, &tracks);
        assert_eq!(view.rows, [0, 2]);
        assert_eq!(tracks.iter().map(|t| t.id).collect::<Vec<_>>(), [9, 8, 9]);
    }
    #[test]
    fn artist_album_and_numeric_duration_sorts_keep_stable_source_ties() {
        let mut tracks = vec![track(1), track(2), track(3)];
        for (i, t) in tracks.iter_mut().enumerate() {
            t.artist.name = ["Zulu", "alpha", "alpha"][i].into();
            t.album.title = ["beta", "beta", "Alpha"][i].into();
            t.duration = [100, 9, 9][i];
        }
        let mut view = View::default();
        for (sort, expected) in [
            (Sort::Artist, vec![1, 2, 0]),
            (Sort::Album, vec![2, 0, 1]),
            (Sort::Duration, vec![1, 2, 0]),
        ] {
            view.sort = sort;
            view.sync(Some(7), 1, &tracks);
            assert_eq!(view.rows, expected);
        }
        view.query = "ZULU".into();
        view.sync(Some(7), 1, &tracks);
        assert_eq!(view.rows, [0]);
    }
    #[test]
    fn append_replacement_and_account_changes_invalidate_cached_projection() {
        let mut tracks = vec![track(1)];
        let mut view = View::default();
        view.sync(Some(7), 1, &tracks);
        view.query = "needle".into();
        view.sync(Some(7), 1, &tracks);
        assert!(view.rows.is_empty());
        tracks.push(track(2));
        tracks[1].album.title = "NEEDLE".into();
        view.sync(Some(7), 1, &tracks);
        assert_eq!(view.rows, [1]);
        tracks[1].album.title.clear();
        view.invalidate();
        view.sync(Some(7), 1, &tracks);
        assert!(view.rows.is_empty());
        view.sync(Some(8), 1, &tracks);
        assert_eq!(view.rows, [0, 1]);
        assert!(!view.active());
        view.query = "absent".into();
        view.sync(Some(8), 2, &tracks);
        assert!(!view.active());
    }
}
