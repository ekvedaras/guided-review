# Change notes

One file per change a user can see. The release notes are the notes added since the previous `v*` tag (`scripts/release-notes`). Once the release workflow has made the draft, it deletes those notes from `main`; they stay readable at the tag.

- Name: `<kind>-<slug>.md`, where kind is `added`, `changed`, `fixed` or `removed`. Example: `added-agent-comments.md`.
- Body: one or two sentences for a user, not a maintainer. Say what they can now do or what now works, not how the code does it. Markdown is fine.
- One PR can add many notes, one per change. A change users cannot see (refactor, lints, tests, CI, snapshots) gets none.

CI fails a PR that touches `src/` or `skills/` with no new note, unless its body has a line `changelog: none`.
