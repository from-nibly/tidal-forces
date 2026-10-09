use std::collections::BTreeSet;

/// Positions identify loaded occurrences, not track IDs. Sorting/filtering must
/// map visible rows back to these source positions rather than reorder the source.
#[derive(Default)]
pub(super) struct Selection {
    pub rows: BTreeSet<usize>,
    pub focus: Option<usize>,
    pub pending_focus: Option<usize>,
    anchor: Option<usize>,
    scope: Option<(Option<u64>, u64)>,
    loaded: usize,
}
impl Selection {
    pub fn sync(&mut self, user: Option<u64>, generation: u64, loaded: usize) {
        if self.scope != Some((user, generation)) || loaded < self.loaded {
            self.clear();
        }
        self.scope = Some((user, generation));
        self.loaded = loaded;
    }
    pub fn clear(&mut self) {
        self.rows.clear();
        self.anchor = None;
        self.focus = None;
        self.pending_focus = None;
    }
    pub fn select(&mut self, row: usize, toggle: bool, range: bool) {
        if row >= self.loaded {
            return;
        }
        if range {
            let anchor = self.anchor.unwrap_or(row);
            if !toggle {
                self.rows.clear();
            }
            self.rows.extend(anchor.min(row)..=anchor.max(row));
            self.anchor = Some(anchor);
        } else {
            if !toggle {
                self.rows.clear();
            }
            if !self.rows.insert(row) {
                self.rows.remove(&row);
            }
            self.anchor = Some(row);
        }
        self.focus = Some(row);
    }
    pub fn all(&mut self) {
        self.rows = (0..self.loaded).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges_toggles_and_append_preserve_source_occurrences() {
        let mut s = Selection::default();
        s.sync(Some(7), 1, 6);
        s.select(1, false, false);
        s.select(3, true, false);
        assert_eq!(s.rows.iter().copied().collect::<Vec<_>>(), [1, 3]);
        s.select(5, false, true);
        assert_eq!(s.rows.iter().copied().collect::<Vec<_>>(), [3, 4, 5]);
        s.select(4, true, false);
        assert_eq!(s.rows.iter().copied().collect::<Vec<_>>(), [3, 5]);
        s.sync(Some(7), 1, 8);
        assert_eq!(s.rows.len(), 2);
        s.all();
        assert_eq!(s.rows.len(), 8);
        s.sync(Some(7), 2, 8);
        assert!(s.rows.is_empty());
        s.all();
        s.sync(Some(8), 2, 8);
        assert!(s.rows.is_empty());
        s.all();
        s.sync(Some(8), 2, 2);
        assert!(s.rows.is_empty());
        s.select(7, false, false);
        assert!(s.rows.is_empty());
    }
}
