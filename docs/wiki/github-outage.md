# GitHub outage

2026-11-12. During a GitHub 503 the sidebar listed three long red problems and three failure toasts, each repeating the PR URL and "http status: 503", and none went away until the window was focused again. Code (names since #140): `src/viewer/host_outage.rs`, `HostError::NotAnswering` in `src/viewer/ports.rs`, the classification in `src/github.rs`, `Problem::HostNotAnswering` in `src/viewer/problems.rs`, the retries in `Review::could_not_read`.

## One problem, told once

A server error (5xx), a timeout or no connection is an outage; 4xx, a refused token and a GraphQL error are not, and keep their own problem. `impl From<anyhow::Error> for HostError` tells them apart by downcasting to `ureq::Error`, not by matching text, so every call that passes its error through `?` is classified in one place, whatever context it added.

Every read that fails that way adds itself to the one outage problem, with a failure count per read. The headline names the cause and the wait; the line under it lists the reads that may be old. It ends when each read it holds goes through, not when any one does, so it does not flap while one read is still failing.

Its toast is neutral, so it leaves by itself after a few seconds: an outage is not the reviewer's failure, and the sidebar keeps the problem. It is told once for the whole outage, however its cause, wait and reads change (`announcements`). "GitHub is back" is a success toast when the problem ends.

## Retries

A failed read is wanted again in 30 s, then 1 min, 2 min, 4 min, and every 5 min after, counted per read. Focus still reads at once. The agenda replaces a later try when one is wanted again, so a focus read that fails sets the next wait from its own count.

- 2026-11-17, #94: the PR status (draft, checks) is a read like the others: `Affected::PrStatus`, retried by `WorkKind::PullPrStatus`, kept as last read while it fails. Its writes (convert to draft, mark ready) are not an outage case: a failure is `Problem::ReadinessNotChanged`, whose danger toast stays until dismissed. Checks that are running are read again every 30 s (`CHECKS_RECHECK`) by `want_in`, which an outage's backoff replaces and a read that goes through restores; once none runs the polling ends. Polling does not stop while the window is in the back: the app has no notion of that yet (open).
- 2026-11-17, #95: auto-merge and Merge now write like draft / ready: `WorkKind::ChangeMerging`, a failure is `Problem::MergingNotChanged` (a danger toast that stays, and a sidebar line), a write that goes through is read back with `PullPrStatus`. The decided "refusals are toasts" is read as GitHub's refusal known before asking (the viewer may not, the repo does not allow auto-merge, the PR is closed): a neutral toast and nothing sent. A refusal GitHub gives on the write itself is a failure like any other write's.

## Open

- ureq has no timeout by default (`Timeout` cannot happen unless one is set), so a hung connection never becomes an outage. No timeout is configured yet.
- A write that fails during an outage still reports as a failure of that action, with the short "GitHub is not answering (503)".
