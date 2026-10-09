use crate::{library::FavoriteKind, model::RadioSeed};

#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Page {
    Home,
    Search,
    Library(FavoriteKind),
    Playlists,
    Settings,
    Track(u64),
    Artist(u64),
    Mix {
        id: String,
        title: String,
    },
    Radio(RadioSeed),
    Collection {
        kind: String,
        id: String,
        title: String,
    },
}

impl Page {
    pub fn is_library(&self) -> bool {
        matches!(self, Self::Library(_) | Self::Playlists)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Location {
    pub page: Page,
    pub query: String,
}

#[derive(Default)]
pub(super) struct Navigation {
    back: Vec<Location>,
    forward: Vec<Location>,
}

impl Navigation {
    pub fn visit(&mut self, from: Location, to: &Location) {
        if &from != to {
            if self.back.len() == 64 {
                self.back.remove(0);
            }
            self.back.push(from);
            self.forward.clear();
        }
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }
    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn back(&mut self, current: Location) -> Option<Location> {
        let previous = self.back.pop()?;
        self.forward.push(current);
        Some(previous)
    }

    pub fn forward(&mut self, current: Location) -> Option<Location> {
        let next = self.forward.pop()?;
        self.back.push(current);
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn search(query: &str) -> Location {
        Location {
            page: Page::Search,
            query: query.into(),
        }
    }

    #[test]
    fn preserves_searches_and_discards_forward_on_a_new_route() {
        let mut nav = Navigation::default();
        nav.visit(search("one"), &search("two"));
        nav.visit(search("two"), &search("two"));
        assert_eq!(nav.back(search("two")), Some(search("one")));
        assert!(!nav.can_back());
        assert_eq!(nav.forward(search("one")), Some(search("two")));
        assert_eq!(nav.back(search("two")), Some(search("one")));
        nav.visit(search("one"), &search("three"));
        assert!(!nav.can_forward());
    }

    #[test]
    fn history_is_bounded() {
        let mut nav = Navigation::default();
        for n in 0..100 {
            nav.visit(search(&n.to_string()), &search(&(n + 1).to_string()));
        }
        assert_eq!(nav.back.len(), 64);
        assert_eq!(nav.back.first().unwrap(), &search("36"));
    }
}
