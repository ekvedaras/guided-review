# Wiki

Notes for agents and people who work on this repo: decisions, the reasons behind them, and what is known to be open. Plans and tickets live in GitHub issues, not here. Each page is dated, so check the commit a claim cites before you trust it. The rules themselves are in [CODING_STANDARDS.md](../../CODING_STANDARDS.md) and [AGENTS.md](../../AGENTS.md); the wiki cites them and does not repeat them.

- [Panic and error policy](panic-policy.md): why `unwrap`, `expect` and `panic!` are limited the way they are.
- [Where work is tracked](delivery.md): plans and tickets in GitHub issues, the wiki keeps decisions.
- [Agent runs the app starts](agent-runs.md): the silent refresh failure and its lessons, and the 2026-11-06 decisions on the tour agent, the PR switcher and one window; why the first app-started agents could not find `guided-review`; why an Ask handed to the reviewer's own agent may change files; the 2026-11-10 reversal that lets the tour agent change the PR's code; why Save in Settings reopens the PR only once no Ask or hand-off is in flight, and holds agent replies meanwhile; the 2026-11-10 change that lets the one window hold only Settings when no PR is open.
- [Rust quality audit, 2026-11](rust-audit-2026-11.md): what the audit set out to do, the lasting outcomes and why, and what is still open.
- [PR notes](pr-notes.md): the 2026-11-11 buttons that put a diagram, a summary picture or the whole reviewer summary in the PR description, why pictures go up through GitHub's undocumented drag-and-drop store, and how a note already there is found.
- [Updates](updates.md): the 2026-11-11 one-click update, why only a release install updates itself, why an update moves links instead of writing over the binary, and how often the app checks.
- [Review UX](review-ux.md): the 2026-11-10 changes that lead a reviewer through a tour: one table of shortcuts and the shortcuts sheet, actions in the palette, one gap row for hidden lines, the summary as a start page, the finish moment, toasts, and Settings as a sheet over the whole window, which retired the sidebar-click-while-Settings rule.
