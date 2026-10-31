# Native Pulse binding patch

`libpulse-binding/` is the unmodified published `libpulse-binding` 2.30.1 crate
except for `src/operation.rs`:

- Retain non-null callback pointers (the upstream condition was reversed), so
  cancellation releases callback captures rather than leaking them.
- Do not release a completed operation's callback again: its completion proxy
  already owns that cleanup.

The application cancels bounded native queries on timeout. Dropped live operations
also need cancellation by their owner; this patch intentionally preserves upstream
Drop semantics rather than silently canceling operations whose callbacks should
continue. Regression checks run only against the private null daemon via
`tests/output-pulse.py`; they check cancellation and cancel-after-completion.

Upstream: https://github.com/jnqnfe/pulse-binding-rust
Crate: https://crates.io/crates/libpulse-binding/2.30.1
Original MIT and Apache-2.0 licenses are retained in the vendored directory.
Remove this override when an audited upstream release provides equivalent fixes.
