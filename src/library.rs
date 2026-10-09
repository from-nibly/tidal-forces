use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FavoriteKind {
    Tracks,
    Albums,
    Artists,
}

impl FavoriteKind {
    pub const ALL: [Self; 3] = [Self::Tracks, Self::Albums, Self::Artists];

    pub fn path(self) -> &'static str {
        match self {
            Self::Tracks => "tracks",
            Self::Albums => "albums",
            Self::Artists => "artists",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Tracks => "Tracks",
            Self::Albums => "Albums",
            Self::Artists => "Artists",
        }
    }

    pub fn form_key(self) -> &'static str {
        match self {
            Self::Tracks => "trackId",
            Self::Albums => "albumId",
            Self::Artists => "artistId",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Favorite {
    pub kind: FavoriteKind,
    pub id: u64,
}

impl Favorite {
    pub fn new(kind: FavoriteKind, id: u64) -> Self {
        Self { kind, id }
    }
}

/// Unknown membership is not the same as an unfavorited item. Only account reads
/// and confirmed writes establish state; failed writes invalidate it.
#[derive(Default)]
pub struct Favorites {
    known: HashMap<Favorite, bool>,
    pending: HashSet<Favorite>,
    complete: HashSet<FavoriteKind>,
}

impl Favorites {
    pub fn state(&self, item: Favorite) -> Option<bool> {
        self.known
            .get(&item)
            .copied()
            .or_else(|| self.complete.contains(&item.kind).then_some(false))
    }

    pub fn pending(&self, item: Favorite) -> bool {
        self.pending.contains(&item)
    }

    pub fn begin(&mut self, item: Favorite) -> bool {
        item.id != 0 && self.pending.insert(item)
    }

    pub fn finish(&mut self, item: Favorite, result: Option<bool>) {
        self.pending.remove(&item);
        if let Some(saved) = result {
            self.known.insert(item, saved);
        } else {
            self.known.remove(&item);
            self.complete.remove(&item.kind);
        }
    }

    pub fn observe(&mut self, item: Favorite) {
        if !self.pending(item) {
            self.known.insert(item, true);
        }
    }

    pub fn invalidate(&mut self, kind: FavoriteKind) {
        self.known.retain(|item, _| item.kind != kind);
        self.complete.remove(&kind);
    }

    pub fn mark_complete(&mut self, kind: FavoriteKind) {
        self.complete.insert(kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_not_false_and_only_confirmed_writes_change_membership() {
        let item = Favorite::new(FavoriteKind::Tracks, 9);
        let mut favorites = Favorites::default();
        assert_eq!(favorites.state(item), None);
        assert!(favorites.begin(item));
        assert!(!favorites.begin(item));
        assert_eq!(favorites.state(item), None);
        favorites.finish(item, Some(true));
        assert_eq!(favorites.state(item), Some(true));
        assert!(favorites.begin(item));
        favorites.observe(item);
        assert_eq!(favorites.state(item), Some(true));
        favorites.finish(item, None);
        assert_eq!(favorites.state(item), None);
        assert!(!favorites.pending(item));
        favorites.finish(item, Some(false));
        assert_eq!(favorites.state(item), Some(false));
    }

    #[test]
    fn absence_is_known_only_after_a_complete_read_and_invalidated_on_uncertain_writes() {
        let saved = Favorite::new(FavoriteKind::Tracks, 9);
        let missing = Favorite::new(FavoriteKind::Tracks, 10);
        let mut favorites = Favorites::default();
        favorites.observe(saved);
        assert_eq!(favorites.state(missing), None);
        favorites.mark_complete(FavoriteKind::Tracks);
        assert_eq!(favorites.state(missing), Some(false));
        assert_eq!(favorites.state(saved), Some(true));
        favorites.begin(missing);
        favorites.finish(missing, None);
        assert_eq!(favorites.state(missing), None);
        assert_eq!(favorites.state(saved), Some(true));
    }

    #[test]
    fn ids_are_namespaced_and_refresh_does_not_unlock_pending_writes() {
        let track = Favorite::new(FavoriteKind::Tracks, 9);
        let album = Favorite::new(FavoriteKind::Albums, 9);
        let mut favorites = Favorites::default();
        favorites.observe(track);
        assert_eq!(favorites.state(album), None);
        favorites.observe(album);
        favorites.begin(track);
        favorites.invalidate(FavoriteKind::Tracks);
        assert!(favorites.pending(track));
        assert_eq!(favorites.state(track), None);
        assert_eq!(favorites.state(album), Some(true));
        assert!(!favorites.begin(Favorite::new(FavoriteKind::Tracks, 0)));
    }
}
