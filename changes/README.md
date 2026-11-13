# Change notes

One file per change a user can see. The release notes are the notes added since the previous `v*` tag (`scripts/release-notes`). When the release is published, the `release-published` workflow deletes those notes from `main`; they stay readable at the tag. Until then the tag can still move to a fix.

- Name: `<kind>-<slug>.md`, where kind is `added`, `changed`, `fixed` or `removed`. Example: `added-agent-comments.md`.
- Body: one or two sentences for a user, not a maintainer. Say what they can now do or what now works, not how the code does it. Markdown is fine.
- Add the note in the same PR as the change: its line in the release notes ends on the PR that added the note and that PR's author, like `(#43 by @login)`.
- One PR can add many notes, one per change. A change users cannot see (refactor, lints, tests, CI, snapshots) gets none.

CI fails a PR that touches `src/` or `skills/` with no new note, unless its body has a line `changelog: none`.
