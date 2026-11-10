//! Private P7b target-boundary prototype. No device IO or alternate queue.
//! Kept test-only until the native worker and local-output release handshake are wired.
use crate::cast_media::End;
use crate::queue::Queue;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Context {
    user: u64,
    play_generation: u64,
    occurrence: u64,
    track: u64,
}
impl Context {
    fn current(user: u64, play_generation: u64, queue: &Queue) -> Option<Self> {
        let entry = queue.current_entry()?;
        (user != 0 && entry.track.id != 0).then_some(Self {
            user,
            play_generation,
            occurrence: entry.occurrence,
            track: entry.track.id,
        })
    }
    fn matches_queue(self, queue: &Queue) -> bool {
        queue
            .current_entry()
            .is_some_and(|e| e.occurrence == self.occurrence && e.track.id == self.track)
    }
}
// Identity contains account/occurrence data; do not derive Debug or serialize it.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Ticket {
    serial: u64,
    connection: u64,
    receiver: u64,
    context: Context,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Local,
    QuiescingLocal,
    Preparing,
    Loading,
    Remote,
    RemoteIdle,
    Unconfirmed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Advance {
    NextPlaying,
    NextPaused,
    Ended,
}
#[derive(Default)]
struct Target {
    selected: Option<u64>,
    serial: u64,
    active: Option<Ticket>,
    phase: Phase,
    release_confirmed: bool,
}
impl Target {
    fn select(&mut self, receiver: u64) {
        self.selected = (receiver != 0).then_some(receiver);
    }
    fn issue(
        &mut self,
        receiver: u64,
        context: Context,
        connection: Option<u64>,
    ) -> Option<Ticket> {
        let Some(serial) = self.serial.checked_add(1) else {
            self.phase = Phase::Unconfirmed;
            return None;
        };
        self.serial = serial;
        let ticket = Ticket {
            serial,
            connection: connection.unwrap_or(serial),
            receiver,
            context,
        };
        self.active = Some(ticket);
        Some(ticket)
    }
    fn begin_transfer(&mut self, context: Context) -> Option<Ticket> {
        if self.phase != Phase::Local {
            return None;
        }
        let ticket = self.issue(self.selected?, context, None)?;
        self.phase = Phase::QuiescingLocal;
        self.release_confirmed = false;
        Some(ticket)
    }
    fn valid(&mut self, ticket: Ticket, current: Context) -> bool {
        if self.active != Some(ticket) {
            return false;
        }
        if ticket.context != current {
            self.invalidate();
            return false;
        }
        true
    }
    fn local_quiesced(&mut self, ticket: Ticket, current: Context, confirmed: bool) -> bool {
        if !self.valid(ticket, current) || self.phase != Phase::QuiescingLocal {
            return false;
        }
        self.phase = if confirmed {
            Phase::Preparing
        } else {
            Phase::Unconfirmed
        };
        confirmed
    }
    fn authorize_load(&mut self, ticket: Ticket, current: Context) -> bool {
        if !self.valid(ticket, current) || self.phase != Phase::Preparing {
            return false;
        }
        self.phase = Phase::Loading;
        true
    }
    fn remote_confirmed(&mut self, ticket: Ticket, current: Context) -> bool {
        if !self.valid(ticket, current) || self.phase != Phase::Loading {
            return false;
        }
        self.phase = Phase::Remote;
        true
    }
    fn accept_end(
        &mut self,
        ticket: Ticket,
        current: Context,
        queue: &mut Queue,
        play_intent: bool,
        end: End,
    ) -> Option<Advance> {
        if end == End::Finished {
            return self.finished(ticket, current, queue, play_intent);
        }
        if self.valid(ticket, current) {
            self.invalidate();
        }
        None
    }
    fn finished(
        &mut self,
        ticket: Ticket,
        current: Context,
        queue: &mut Queue,
        play_intent: bool,
    ) -> Option<Advance> {
        if !self.valid(ticket, current) || self.phase != Phase::Remote {
            return None;
        }
        if !current.matches_queue(queue) {
            self.invalidate();
            return None;
        }
        self.phase = Phase::RemoteIdle;
        Some(if queue.advance(true) {
            if play_intent {
                Advance::NextPlaying
            } else {
                Advance::NextPaused
            }
        } else {
            Advance::Ended
        })
    }
    fn prepare_next(&mut self, current: Context) -> Option<Ticket> {
        if self.phase != Phase::RemoteIdle {
            return None;
        }
        let old = self.active?;
        if old.context.user != current.user {
            self.invalidate();
            return None;
        }
        // A newly selected device never silently inherits the active receiver's queue.
        let ticket = self.issue(old.receiver, current, Some(old.connection))?;
        self.phase = Phase::Preparing;
        Some(ticket)
    }
    fn connection_failed(&mut self, connection: u64) -> bool {
        if self
            .active
            .is_none_or(|ticket| ticket.connection != connection)
        {
            return false;
        }
        // A connection fault also fences a newer pending LOAD on that connection.
        self.invalidate();
        true
    }
    fn invalidate(&mut self) {
        self.phase = Phase::Unconfirmed;
        self.release_confirmed = false;
    }
    fn released(&mut self, connection: u64) -> bool {
        if self
            .active
            .is_none_or(|ticket| ticket.connection != connection)
            || self.phase != Phase::Unconfirmed
        {
            return false;
        }
        self.release_confirmed = true;
        true
    }
    fn return_local(&mut self) -> bool {
        if self.phase != Phase::Unconfirmed || !self.release_confirmed {
            return false;
        }
        self.active = None;
        self.phase = Phase::Local;
        self.release_confirmed = false;
        // This authorizes a target, not Play. The application must remain paused.
        true
    }
    fn local_play_allowed(&self) -> bool {
        self.phase == Phase::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn queue(ids: &[u64]) -> Queue {
        let mut q = Queue::default();
        q.start(
            ids.iter()
                .map(|id| crate::model::Track {
                    id: *id,
                    duration: 120,
                    ..Default::default()
                })
                .collect(),
            0,
            "Synthetic".into(),
            None,
        )
        .unwrap();
        q
    }
    fn context(q: &Queue) -> Context {
        Context::current(1, 10, q).unwrap()
    }
    fn connected(t: &mut Target, c: Context) -> Ticket {
        t.select(1);
        let ticket = t.begin_transfer(c).unwrap();
        assert!(t.local_quiesced(ticket, c, true));
        assert!(t.authorize_load(ticket, c));
        assert!(t.remote_confirmed(ticket, c));
        ticket
    }
    #[test]
    fn selection_does_nothing_and_load_requires_confirmed_local_release() {
        let q = queue(&[1]);
        let c = context(&q);
        let mut t = Target::default();
        t.select(1);
        assert_eq!(t.phase, Phase::Local);
        assert!(t.active.is_none());
        let ticket = t.begin_transfer(c).unwrap();
        assert!(!t.local_play_allowed());
        assert!(!t.authorize_load(ticket, c));
        assert!(!t.remote_confirmed(ticket, c));
        assert!(t.local_quiesced(ticket, c, true));
        assert!(t.authorize_load(ticket, c));
        assert!(!t.authorize_load(ticket, c));
        assert!(!t.local_play_allowed());
    }
    #[test]
    fn duplicate_tracks_use_occurrences_and_finish_advances_exactly_once() {
        let mut q = queue(&[1, 1, 2]);
        let c = context(&q);
        let mut t = Target::default();
        let first = connected(&mut t, c);
        assert_eq!(
            t.finished(first, c, &mut q, true),
            Some(Advance::NextPlaying)
        );
        assert_eq!(q.current().unwrap().id, 1);
        assert_ne!(q.current_entry().unwrap().occurrence, c.occurrence);
        let revision = q.revision();
        assert_eq!(t.finished(first, c, &mut q, true), None);
        assert_eq!(q.revision(), revision);
        let next = Context::current(1, 11, &q).unwrap();
        let second = t.prepare_next(next).unwrap();
        assert!(second != first);
        assert!(t.authorize_load(second, next) && t.remote_confirmed(second, next));
        assert_eq!(t.finished(first, c, &mut q, true), None);
        assert_eq!(
            t.finished(second, next, &mut q, true),
            Some(Advance::NextPlaying)
        );
        assert_eq!(q.current().unwrap().id, 2);
    }
    #[test]
    fn repeat_one_gets_a_new_load_ticket_even_with_same_occurrence() {
        let mut q = queue(&[1, 2]);
        q.set_repeat(crate::queue::Repeat::Track);
        let c = context(&q);
        let mut t = Target::default();
        let first = connected(&mut t, c);
        assert_eq!(
            t.finished(first, c, &mut q, true),
            Some(Advance::NextPlaying)
        );
        assert_eq!(q.current_entry().unwrap().occurrence, c.occurrence);
        let second = t.prepare_next(c).unwrap();
        assert!(second != first);
        assert!(t.authorize_load(second, c) && t.remote_confirmed(second, c));
        assert_eq!(t.finished(first, c, &mut q, true), None);
    }
    #[test]
    fn account_seek_and_occurrence_changes_reject_late_effects() {
        for changed in [
            Context {
                user: 2,
                ..context(&queue(&[1]))
            },
            Context {
                play_generation: 11,
                ..context(&queue(&[1]))
            },
            Context {
                occurrence: 99,
                ..context(&queue(&[1]))
            },
        ] {
            let mut q = queue(&[1, 2]);
            let c = context(&q);
            let mut t = Target::default();
            let ticket = connected(&mut t, c);
            assert_eq!(t.finished(ticket, changed, &mut q, true), None);
            assert_eq!(q.current().unwrap().id, 1);
            assert_eq!(t.phase, Phase::Unconfirmed);
            assert!(!t.local_play_allowed());
        }
    }
    #[test]
    fn ambiguity_requires_owned_release_and_explicit_local_return() {
        let q = queue(&[1]);
        let c = context(&q);
        let mut t = Target::default();
        let ticket = connected(&mut t, c);
        t.invalidate();
        assert!(!t.return_local());
        assert!(!t.local_play_allowed());
        let mut stale = ticket;
        stale.connection += 1;
        assert!(!t.released(stale.connection));
        assert!(t.released(ticket.connection));
        assert!(!t.local_play_allowed());
        assert!(t.return_local());
        let next = t.begin_transfer(c).unwrap();
        assert!(!t.released(ticket.connection));
        assert!(!t.remote_confirmed(ticket, c));
        assert!(next != ticket);
    }
    #[test]
    fn paused_intent_and_new_selection_do_not_start_another_output() {
        let mut q = queue(&[1, 2]);
        let c = context(&q);
        let mut t = Target::default();
        let ticket = connected(&mut t, c);
        t.select(2);
        assert_eq!(
            t.finished(ticket, c, &mut q, false),
            Some(Advance::NextPaused)
        );
        assert_eq!(t.phase, Phase::RemoteIdle);
        assert!(!t.local_play_allowed());
        let next = t
            .prepare_next(Context::current(1, 11, &q).unwrap())
            .unwrap();
        assert_eq!(next.receiver, 1);
    }
    #[test]
    fn connection_fault_fences_pending_next_but_not_a_later_connection() {
        let mut q = queue(&[1, 2]);
        let c = context(&q);
        let mut t = Target::default();
        let first = connected(&mut t, c);
        assert_eq!(
            t.finished(first, c, &mut q, true),
            Some(Advance::NextPlaying)
        );
        let current = Context::current(1, 11, &q).unwrap();
        let next = t.prepare_next(current).unwrap();
        assert_eq!(first.connection, next.connection);
        assert!(t.connection_failed(first.connection));
        assert!(!t.authorize_load(next, current));
        assert!(t.released(next.connection));
        assert!(t.return_local());
        let fresh = connected(&mut t, current);
        assert_ne!(first.connection, fresh.connection);
        assert!(!t.connection_failed(first.connection));
        assert_eq!(t.phase, Phase::Remote);
    }
    #[test]
    fn only_explicit_finish_advances_the_shared_queue() {
        for end in [
            End::Cancelled,
            End::Interrupted,
            End::Error,
            End::UnknownIdle,
            End::OwnershipLost,
            End::Ambiguous,
        ] {
            let mut q = queue(&[1, 2]);
            let c = context(&q);
            let mut t = Target::default();
            let ticket = connected(&mut t, c);
            assert_eq!(t.accept_end(ticket, c, &mut q, true, end), None);
            assert_eq!(q.current().unwrap().id, 1);
            assert!(!t.local_play_allowed());
            assert_eq!(t.accept_end(ticket, c, &mut q, true, End::Finished), None);
        }
        let mut q = queue(&[1, 2]);
        let c = context(&q);
        let mut t = Target::default();
        let ticket = connected(&mut t, c);
        assert_eq!(
            t.accept_end(ticket, c, &mut q, true, End::Finished),
            Some(Advance::NextPlaying)
        );
    }
    #[test]
    fn failed_quiescence_and_counter_exhaustion_fail_closed() {
        let q = queue(&[1]);
        let c = context(&q);
        let mut t = Target::default();
        t.select(1);
        let ticket = t.begin_transfer(c).unwrap();
        assert!(!t.local_quiesced(ticket, c, false));
        assert!(!t.authorize_load(ticket, c));
        assert!(!t.local_play_allowed());
        let mut t = Target {
            serial: u64::MAX,
            ..Default::default()
        };
        t.select(1);
        assert!(t.begin_transfer(c).is_none());
        assert!(!t.local_play_allowed());
    }
}
