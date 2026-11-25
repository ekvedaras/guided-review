# Code host capabilities

2026-11-25, issue #140 step 2.

## Decision

`CodeHost` (`src/viewer/ports.rs`) is core ports as supertraits, plus one accessor per capability that returns `Option<&dyn Port>`:

| Capability | Port | Accessor | Why it is optional |
|---|---|---|---|
| Viewed marks | `ViewedFilesPort` | `viewed_marks` | GitLab keeps its "viewed" checkbox in the browser, with no API (not checked against GitLab's current API). |
| Merging | `MergingPort` | `merging` | `MergeChange` is GitHub's auto-merge shape; a host may merge only on its web page. |
| Reactions on a PR | `PullRequestReactionsPort` | `pull_request_reactions` | Bitbucket and Azure DevOps have no emoji on a PR (not checked). It only carries the reviewing mark. |
| Picture uploads | `PictureUploadPort` | `picture_uploads` | GitHub's store is undocumented ([PR notes](pr-notes.md)); another host may have none. |
| Issue tracker | `IssueDetailsPort` | `issue_tracker` | A repo's issues may live in Jira, not on the host. |

Core, every host of PRs does it: `ChangeTipsPort`, `PullsOfBranchPort`, `PullRequestDescriptionPort`, `PullRequestOriginPort`, `PullRequestStatusPort` (status, checks, draft), `ReviewThreadsPort`, `ReviewWritesPort`. GitLab has each: diff refs, MRs by source branch, the description, draft MRs, pipelines, discussions with resolve, draft notes published as one batch, and approve (from my knowledge of GitLab's API, not checked against its docs). `change_merging` moved out of `PullRequestStatusPort` into `MergingPort`.

`NoHost`, a local review's host, returns `None` for every capability; its core ports still refuse, since each takes a PR URL a local review does not have.

## Why ports and not a flag list

A flag list beside the ports (`supports_viewed_marks: true`) can disagree with them: the flag says yes and the port does nothing, or the other way. An optional port cannot: the host has the port or it does not, and the work that uses it reaches it only through the `Option`. Work for a capability the host lacks fails with `HostError::not_offered`, a `NoAccess` refusal, which the review never asks for.

## How the review learns it

`Review::host_is(&dyn CodeHost)` reads each accessor once and tells the part that shows it: `ViewedSync::without_host_marks`, `ReviewingMark::without_reactions`, `PrControls::without_merging` (no merge button, `Shift+M` or palette action), `PrNotes::without_picture_uploads` (no note that shows a picture), `RelatedIssues::without_tracker` (no hover details). The review's work is planned on the UI thread and run with the ports lent on another, so the review cannot hold the port; it holds what the port's absence means for each part.

`open_review` calls it on the opened review, and the session on every review that replaces the shown one (re-pin, refresh, reload, switch to guided, a snapshot's staged review). A review opened with `Review::open` and never told offers every capability, as a GitHub PR does, which keeps tests of a PR short.

## PrControls keeps its fields

`PrControls` now keeps `merging` to itself (`None` on a host with no merging), so deslop-please sees it guard its fields and reported two field leaks it had hidden: `status` (`status()` lent it, `status_is` wrote it) and `merge_method` (`prefer_merge_method` wrote it). Fixed, not allowed:

- `PrControls::status_is` says when to read the status again (while checks run), the rule `ReviewCore` held; `ReviewCore::pr_status` hands out a copy, not a borrow.
- The merge method is the reviewer's setting, not the PR's state: `ReviewCore` holds it and passes it to `PrControls::ask_to_change_merging`. `ReviewCore::read_merge_method_from(&Config)` picks the PR repo's method, so `ReviewBuilder` no longer names it.

## Left open

- Comment reactions (the hand-off's seen mark) are still in `ReviewThreadsPort`; a host with no reactions on comments would need them split out too.
- `PrStatus` holds GitHub's `Merging` (merge state, auto-merge, merge methods). With no `MergingPort` the review hides it, but a GitLab status would have to fill it or the type would need a split.
- `ReviewVerdict::RequestChanges` assumes the host has it; GitLab added a request-changes state late (not checked).
