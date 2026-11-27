# Tests seen red first

2026-11-24. Test-first was a rule in [AGENTS.md](../../AGENTS.md) (Tests) with no trace of having been followed. A retro of the agent sessions that built the tickets of issue #182 found tests that were never red first. Some counted a `todo!()` panic as red: one `todo!()` in a shared function failed 64 unrelated tests, which says nothing about the new test's assertion. Nothing recorded a red run, so skipping it cost one line in a report.

## Decisions

- `scripts/red <exact-test-name>...` runs only those tests, refuses when the build fails, and appends the name, the failure message, the date and HEAD to `red-log` in the git directory (so it is not committed) for each test that failed on something other than a `todo!()` or `unimplemented!()` panic. A passing test is reported as not red; a stub panic is reported as not red too.
- `scripts/check` runs `scripts/red-unrecorded` against the same base as the change-notes check and prints the `#[test]` functions the change adds that have no entry in `red-log`. It is a warning, so a human decides. It is skipped in CI, where `red-log` does not exist.
- AGENTS.md (Tests) asks for one test at a time, a stub that returns a wrong but plausible value, `scripts/red` to record the red, a test that passed first broken on purpose and restored, and a "nothing happens" test made to happen before it counts as red.
- Test data, scenes, fixtures, examples, docs and commit messages use invented names only. On 2026-11-27 the maintainer found names from a private codebase in a scene, a test and two wiki pages, all public. They were replaced, and the rule has no word list: the check is where a name came from (AGENTS.md, Tests).

## Limits

The log proves a test failed on an assertion once, not that the failure was for the right reason, and an agent can still edit the log. The warning is a prompt for the reviewer, not a gate. `scripts/red-unrecorded` matches test functions by their last name segment, so two tests with the same name in different modules count as one.
