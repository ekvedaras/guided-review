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

`HostCapabilities::of(&dyn CodeHost)` reads each accessor once, and `Review::open`, `Review::open_basic` and `ReviewBuilder::new` take it, so no review is built without its host's capabilities. The review tells the part that shows each: `ViewedSync::without_host_marks`, `ReviewingMark::without_reactions`, `PrControls::without_merging` (no merge button, `Shift+M` or palette action), `PrNotes::without_picture_uploads` (no note that shows a picture), `RelatedIssues::without_tracker` (no hover details). The review's work is planned on the UI thread and run with the ports lent on another, so the review cannot hold the port; it holds what the port's absence means for each part.

`open_review` reads it from the opened subject's host (`NoHost` for a local review, so none) and hands it to `ReviewBuilder`, `TourRefresher`, `TourReloader`, `TourSwitcher` and `TourRepinner`, which rebuild the review on re-pin, refresh, reload and switch to guided; the session's snapshot staging reads it from its own host. Nothing tells a review afterwards (`Review::host_is` is gone). Tests pass `support::fake_ports::every_capability()`, or `HostCapabilities::of(&host.without(..))` when the test is about a host that lacks one.

Before this, a review opened any other way offered every capability: a local review opened through `OpenedSubject::open_review` counted its ticks as unsent viewed marks (`opened_subject::a_local_review_opened_as_the_app_opens_it_offers_no_host_viewed_marks`).

## PrControls keeps its fields

`PrControls` now keeps `merging` to itself (`None` on a host with no merging), so deslop-please sees it guard its fields and reported two field leaks it had hidden: `status` (`status()` lent it, `status_is` wrote it) and `merge_method` (`prefer_merge_method` wrote it). Fixed, not allowed:

- `PrControls::status_is` says when to read the status again (while checks run), the rule `ReviewCore` held; `ReviewCore::pr_status` hands out a copy, not a borrow.
- The merge method is the reviewer's setting, not the PR's state: `ReviewCore` holds it and passes it to `PrControls::ask_to_change_merging`. `ReviewCore::read_merge_method_from(&Config)` picks the PR repo's method, so `ReviewBuilder` no longer names it.

## Host contract

Issue #140 step 3, 2026-11-25. `tests/it/host_contract/` holds shared cases per port (viewed marks, review threads), each written once over a builder that makes a host holding a given PR, and `every_case_for!` runs every case against `FakeHost` and the GitHub adapter. GitHub runs against `github_api.rs`, GitHub's GraphQL API for one PR held in memory and answered in GitHub's shapes, two nodes a page, never the network. `capabilities.rs` checks that GitHub offers every capability and `NoHost` none. A new host gets a builder in `hosts.rs` and one more name in `every_case_for!`.

What the contract found, fixed in the fake to match GitHub:

- A reply never showed in `FakeHost`'s threads; a hand-off test had seeded the reply itself to make up for it.
- `FakeHost` had no way to have no token at all: `TokenRefused` refuses only viewed marks. `HostFault::NoToken` refuses every call.
- `ConnectionLost`, "every write fails", let a viewed mark through.

## URLs and clones behind the host

Issue #140 step 4, 2026-11-25. Before it, four modules outside the adapter knew GitHub's web addresses: `pull_request` read and wrote PR links and git remotes, `issue` issue links, `viewer::code_link` file links, `local_git` the `refs/pull/N/head` ref, and the app built `{pr}/checks` itself. The clone offer ran `gh repo clone` through `GhCloner`, picked by the session whatever the PR's host.

- `PullRequestUrl` stays the neutral PR identity and gains `host: HostSite`, which host the PR is on, read from its link. `HostSite::GitHub` is the one host; `HostSite::name` is the word the UI shows.
- `host_web` (new) is a PR's host on the web: the PR's page, which is `PullRequestUrl`'s text form and so its subject key, its checks page, the ref its head is fetched by, a file at a commit (`HostSite::file_page`, `file_of_link`), and the repo a git remote points at. Each is a `match` on `HostSite` that asks the host's own shapes, so a second host is a compile error at every place it must answer.
- `github_web` (new) is GitHub's shapes. It takes and gives a PR's parts (repo, number), not `PullRequestUrl`: when it built and read `PullRequestUrl` while `PullRequestUrl`'s text form asked it, deslop-please reported the five as a tangle. It is a top-level module, not `github::web`, since `github` sits above `viewer` (it implements the viewer's ports) and these shapes sit below it (`viewer::code_link` reaches them through `host_web`).
- `url_path` (new) percent-encodes a path in a link; it held twice, in `viewer::code_link` and `ide`.
- A link of a host the app does not read is refused naming the hosts it reads: "not a PR link of a code host guided-review reads (GitHub): <link>"; a command's subject key says "neither a PR link of a code host guided-review reads (GitHub) nor a local change set key".
- Cloning is a core port, `ClonePort`, a supertrait of `CodeHost`: every host's repos can be cloned. GitHub clones with `gh repo clone`; `NoHost` refuses, since a local review reads its own folder; the session clones through the review's host. `RepoCloner` and `GhCloner` are gone.
- `opened_subject::adapter_of(HostSite)` picks the adapter a PR review talks to, for the open and for the move of a local review to a PR.

```
before
  app::layouts::one_step ... builds "{pr}/checks"
  viewer::code_link ........ builds and reads github.com/<repo>/blob/...
  issue .................... builds and reads github.com/<repo>/issues/N
  local_git ................ fetches refs/pull/N/head
  pull_request ............. reads and writes github.com/<repo>/pull/N and git@github.com: remotes
  app::review_session ──▶ github::GhCloner (gh repo clone)

after
  app, viewer::code_link, local_git, local_work_tree
          │
          ▼
  host_web ── match HostSite ──▶ github_web ──▶ url_path
     │                               │
     └────────▶ pull_request ◀───────┘    HostSite, PullRequestUrl { host, repo, number }, RepoName, Sha
  issue ──▶ github_web                    a GitHub issue is GitHub's

  app::review_session ──▶ CodeHost: ClonePort ◀── github (gh repo clone), NoHost (refuses)
  open_review, opened_subject ──▶ adapter_of(HostSite) ──▶ github::GitHub
```

### Does it fit GitLab

From my knowledge of GitLab, not checked against its docs:

- An MR's page is `https://gitlab.com/<group>/<subgroup>/<project>/-/merge_requests/7`, its head ref `refs/merge-requests/7/head`, its pipelines `.../-/merge_requests/7/pipelines`, a file `.../-/blob/<sha>/<path>#L12`. Each fits a `host_web` method.
- A clone is `git clone https://gitlab.com/<group>/<project>.git` (or `glab repo clone`): it fits `ClonePort::clone_repo(repo, folder)`.
- Two shapes do not fit. `RepoName` is `owner/name`, and a GitLab project path has any number of groups. A self-managed GitLab is its own site, so `HostSite` would carry its address and the hosts the app reads would come from the config, not `HostSite::ALL`.

### Left open after step 4

- `RepoName` is two segments, and everything keyed by it has no host level: the state folder (`state/<owner>/<repo>/`, read back as GitHub's in `ReviewStateFile::folders_opened_from_any_repo`), `Codebase`, the clones in `config.toml`.
- `LocalGit::find` and `configured_clone` take a clone whose remote points at the repo on any host the app reads, not only the PR's host.
- A local review's origin keeps its repo but not its host, so what a local review asks of a host still makes `GitHub::from_env()`: the branch's PR (`app::head_branches`), the move to a PR, `agent-reply`, the switcher's PRs (`relevant_pull_requests`).
- Code read without a clone comes through `GitHub::code_source`; `pick_code_source` takes the GitHub adapter.
- The palette's and title bar's "Open on GitHub" and "Open checks on GitHub" keep GitHub in their names and labels; `open_on_github` is also a key name in `[keys]`, which a rename must keep reading.
- `PullRequestUrl::short` writes `owner/repo#7`; GitLab writes `group/project!7`.
- Snapshot sample data (`app::snapshots`, the staged findings' avatars) stays GitHub's.

## Left open

- Tests of a local review other than `local_review.rs` and `local_guided_tour.rs` that go through `Review::open_basic` with `every_capability()` (for example `basic_tour.rs`, `move_to_pr.rs`) build a local review with a PR host's capabilities; they test reading, not capabilities.

- Comment reactions (the hand-off's seen mark) are still in `ReviewThreadsPort`; a host with no reactions on comments would need them split out too.
- `PrStatus` holds GitHub's `Merging` (merge state, auto-merge, merge methods). With no `MergingPort` the review hides it, but a GitLab status would have to fill it or the type would need a split.
- `ReviewVerdict::RequestChanges` assumes the host has it; GitLab added a request-changes state late (not checked).
