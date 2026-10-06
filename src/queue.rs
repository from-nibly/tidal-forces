use crate::model::Track;
use rand::seq::SliceRandom;

#[derive(Default)]
pub struct Queue {
    pub tracks: Vec<Track>,
    pub index: usize,
    pub repeat: bool,
}
impl Queue {
    pub fn current(&self) -> Option<&Track> {
        self.tracks.get(self.index)
    }
    pub fn next(&mut self) -> bool {
        if self.index + 1 < self.tracks.len() {
            self.index += 1;
            true
        } else if self.repeat && !self.tracks.is_empty() {
            self.index = 0;
            true
        } else {
            false
        }
    }
    pub fn previous(&mut self) -> bool {
        if self.index > 0 {
            self.index -= 1;
            true
        } else {
            false
        }
    }
    pub fn shuffle_remaining(&mut self) {
        if self.index + 1 < self.tracks.len() {
            self.tracks[self.index + 1..].shuffle(&mut rand::rng());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_boundaries_and_repeat() {
        let mut q = Queue::default();
        assert!(!q.next());
        assert!(!q.previous());
        q.repeat = true;
        assert!(!q.next());
        q.tracks = (0..3)
            .map(|id| Track {
                id,
                ..Default::default()
            })
            .collect();
        assert!(q.next());
        assert_eq!(q.current().unwrap().id, 1);
        assert!(q.previous());
        assert_eq!(q.current().unwrap().id, 0);
        q.shuffle_remaining();
        assert_eq!(q.current().unwrap().id, 0);
        q.index = 2;
        assert!(q.next());
        assert_eq!(q.index, 0);
        q.repeat = false;
        q.index = 2;
        assert!(!q.next());
    }
}
