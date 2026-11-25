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
| Requests for changes | `ChangeRequestsPort` | `change_requests` | GitLab has it only in recent versions (not checked); see below. |
| Reactions on comments | `CommentReactionsPort` | `comment_reactions` | Bitbucket and Azure DevOps have no emoji on a comment (not checked); see below. |

Core, every host of PRs does it: `ChangeTipsPort`, `PullsOfBranchPort`, `PullRequestDescriptionPort`, `PullRequestOriginPort`, `PullRequestStatusPort` (status, checks, draft), `ReviewThreadsPort`, `ReviewWritesPort`. GitLab has each: diff refs, MRs by source branch, the description, draft MRs, pipelines, discussions with resolve, draft notes published as one batch, and approve (from my knowledge of GitLab's API, not checked against its docs). `change_merging` moved out of `PullRequestStatusPort` into `MergingPort`.

`NoHost`, a local review's host, returns `None` for every capability; its core ports still refuse, since each takes a PR URL a local review does not have.

## Why ports and not a flag list

A flag list beside the ports (`supports_viewed_marks: true`) can disagree with them: the flag says yes and the port does nothing, or the other way. An optional port cannot: the host has the port or it does not, and the work that uses it reaches it only through the `Option`. Work for a capability the host lacks fails with `HostError::NotOffered(Capability)`, which the review never asks for. It is not `NoAccess`, which says the token is missing or refused. Viewed sync settles it as it settles `NoAccess`: sync goes off for the session and the banner names the missing capability.

## How the review learns it

`Capability` (`src/viewer/capability.rs`) names each one, and `HostCapabilities` is the set a host offers. `HostCapabilities::of(&dyn CodeHost)`, written beside `CodeHost` in `ports.rs`, reads each accessor once, and `Review::open`, `Review::open_basic` and `ReviewBuilder::new` take it, so no review is built without its host's capabilities. Each part that shows a capability is built from the set, so none can show one by default: `ViewedSync::for_host`, `ReviewingMark::new` (finished from the start with no reactions), `PrControls::of` (no merge button, `Shift+M` or palette action), `PrNotes::for_host` (no note that shows a picture), `RelatedIssues::of` (no hover details). A new capability is a `Capability` variant and its name, a `CodeHost` accessor and its arm in `HostCapabilities::of`, and the part that asks `has` for it. `capability` sits below `ports` and the parts, since `ports` names the parts' value types and `HostError::NotOffered` names a `Capability`; reading the set from a host is `ports`'s. The review's work is planned on the UI thread and run with the ports lent on another, so the review cannot hold the port; it holds what the port's absence means for each part.

`open_review` reads it from the opened subject's host (`NoHost` for a local review, so none) and hands it to `ReviewBuilder`, `TourRefresher`, `TourReloader`, `TourSwitcher` and `TourRepinner`, which rebuild the review on re-pin, refresh, reload and switch to guided; the session's snapshot staging reads it from its own host. Nothing tells a review afterwards (`Review::host_is` is gone). Tests pass `support::fake_ports::every_capability()`, or `HostCapabilities::of(&host.without(..))` when the test is about a host that lacks one.

Before this, a review opened any other way offered every capability: a local review opened through `OpenedSubject::open_review` counted its ticks as unsent viewed marks (`opened_subject::a_local_review_opened_as_the_app_opens_it_offers_no_host_viewed_marks`).

## PrControls keeps its fields

`PrControls` now keeps `merging` to itself (`None` on a host with no merging), so deslop-please sees it guard its fields and reported two field leaks it had hidden: `status` (`status()` lent it, `status_is` wrote it) and `merge_method` (`prefer_merge_method` wrote it). Fixed, not allowed:

- `PrControls::status_is` says when to read the status again (while checks run), the rule `ReviewCore` held; `ReviewCore::pr_status` hands out a copy, not a borrow. The copy allocates nothing: `PrStatus` is plain values and an `Arc<[CheckRun]>`, so it costs one reference count. The title bar cannot hold a borrow anyway: it reads the status from the session and then lends `cx` mutably to build its buttons, and `main` copied it each frame too (`.cloned()`).
- The merge method is the reviewer's setting, not the PR's state: `ReviewCore` holds it and passes it to `PrControls::ask_to_change_merging`. `ReviewCore::read_merge_method_from(&Config)` picks the PR repo's method, so `ReviewBuilder` no longer names it.

On a rebuild (re-pin, refresh, reload, switch to guided) the new review takes the replaced one's PR controls, what the host said and what the reviewer pressed, through `PrControls::carry_on_from`, which keeps whether merging is offered from the new build. Assigning the replaced controls whole had kept the old build's answer (`pr_controls::a_reopened_pr_review_offers_merging_as_its_own_build_says_whatever_the_replaced_one_offered`).

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
- `PullRequestUrl::short`, how the host writes a reference to the PR in text, is `host_web`'s too (later the same day): GitHub's `owner/repo#7` comes from `github_web::pull_request_reference`; GitLab would write `group/project!7`.
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
- Two shapes did not fit. `RepoName` was `owner/name`, and a GitLab project path has any number of groups; it is a path now (see A repo is a path below). A self-managed GitLab is its own site, so `HostSite` would carry its address and the hosts the app reads would come from the config, not `HostSite::ALL`.

### Left open after step 4

- The switcher's "Loading your pull requests from GitHub..." and "Refreshing from GitHub..." name GitHub while the list is read from every host the app reads.
- The palette's and title bar's "Open on GitHub" and "Open checks on GitHub" keep GitHub in their names and labels; `open_on_github` is also a key name in `[keys]`, which a rename must keep reading.
- Snapshot sample data (`app::snapshots`, the staged findings' avatars) stays GitHub's.

## A local review knows its host

Issue #140, 2026-11-25. Before it, `host_web::repo_of_remote` kept a remote's repo and dropped its host, so a local review knew its repo but not its host, and six places made `GitHub::from_env()` for what a local review asks of a host: `agent-reply`, `OpenedSubject::host`, the app's branch lookups and the switcher's PRs, Move to PR, and the switcher's local rows.

- `HostedRepo` (`pull_request.rs`) is a host and a repo; `HostedRepo::of_remote` reads both from a git remote, `None` for a host the app does not read. A local change set's `origin` is one, and a local tour's pin keeps its `host` beside `repo` (a pin without one is GitHub's, the only host the app read before).
- `PullsOfBranchPort::open_pull_of_branch` takes a `HostedRepo`, as every other port takes a `PullRequestUrl` that names its host, so a caller holding every host can route each look-up.
- `opened_subject::hosts` is the one place a `HostSite` becomes its adapter: `adapter_of` (`Arc<dyn CodeHost>`), `host_of(&ReviewSubject)` (a PR's adapter, or a local review's `NoHost` looking up its branch's PR on its origin's host, none with no origin), and `EveryHost`, which routes look-ups by the repo's or PR's host and lists the switcher's PRs from every `HostSite::ALL`, merged by `RelevantPullRequests::merged`: newest first, a host that fails left out, failing only when every host does, since a reviewer need not have an account on each.
- `RelevantPullRequestsPort` moved into `ports` and became core: every host lists the PRs that involve the viewer (GitLab: merge requests by author, assignee and reviewer, not checked against its docs).
- Only the release downloaders (`app::updater`, `app::language_servers`) make `GitHub::from_env()` outside the adapter and `opened_subject::hosts`: guided-review's and the language servers' releases are on GitHub whatever host a PR is on.

## Code read on the host

Issue #140, 2026-11-25. Reading a PR's code with no clone is `HostedCodePort::code_of(repo)`, a core port and a supertrait of `CodeHost`: every host serves a file at a commit, the diff between two and their merge base (GitLab: its repository files and compare APIs, not checked). `pick_code_source` takes the port, so `opened_subject::hosts` has one mapping, `adapter_of`, and `code_reader_of` is gone; `OpenedPr` holds the PR's `Arc<dyn CodeHost>`, and the PR's title, which a basic tour is made with, moved into `PullRequestDescriptionPort`. Why the host's code is read though a clone was found (its fetch failed) is `pick_code_source`'s, which wraps whatever the host gives, so no adapter carries a fallback reason. `NoHost` refuses: a local review reads its own folder.

## A repo is a path

Issue #140, 2026-11-25. `RepoName` was `owner/name`; a GitLab project path has any number of groups (`group/sub/project`). It is now a namespace of one segment or more and a name, read from `namespace/name` split at the last `/`, every segment non-empty; `owner()` became `namespace()`. GitHub's form is the same text, so every persisted form of a GitHub repo (state and tour folders `<owner>/<repo>`, `server-checkouts/<owner>/<repo>`, `config.toml` keys) is unchanged. `config.toml`'s `[clones]` and `[merge_methods]` keys and Settings' clone rows are parsed as a `RepoName`, not split at the first `/`. A GitHub issue reference (`owner/repo#12`) reads only GitHub's two segments (`github_web::repo_named`), so `a/b/c#12` names no issue, as before.

## A clone is of a repo on a host

Issue #140, 2026-11-25. `LocalGit::find` and `configured_clone` took a `RepoName` and accepted a clone whose remote pointed at that repo on any host the app reads. They take a `HostedRepo` and accept only a remote of that repo on that host, as do `Config::clone_of`, Settings' clone rows and the clone offer, whose line names the host the code comes from. `HostedRepo` has a text form: `<host key>:namespace/name` (`HostSite::key`, `github`), and a bare `namespace/name` is GitHub's, so every `[clones]` key written before reads as GitHub's, and a GitHub repo is still written bare (`clone_offer::a_clone_of_a_github_repo_is_saved_under_its_owner_and_name_as_before_hosts_were_named`). A second host writes its key, since `HostedRepo`'s `Display` is a `match` on the host.

## What a repo keys is keyed by its host

Issue #140, 2026-11-25. The state and tour folders of a PR (`ReviewSubject::files_folder`), `Codebase` (the language servers' answer, server checkout and server index) and `[merge_methods]` were keyed by `RepoName`, and `ReviewStateFile::folders_opened_from_any_repo` read every `state/<owner>/<repo>/` back as GitHub's. Each is keyed by `HostedRepo` now: `Codebase::GitHub(RepoName)` became `Codebase::Hosted(HostedRepo)`, and `HostedRepo::files_folder` is the one place a repo becomes a folder, GitHub's `<owner>/<repo>` as before, so nothing on disk moves. `HostedRepo::of_files_folder` reads a folder back, and `folders_opened_from_any_repo` walks the state folder down to each folder it reads as a repo, so a second host's deeper folders are found once its arm of the `match` names them. Its folders go under `_<host key>/`: GitHub owners cannot start with `_`, as `Codebase`'s `_folders` already counts on. Keys in `config.toml` (`[language_servers]`, `[merge_methods]`) are `HostedRepo`'s text form, bare for GitHub.

```
before                                       after
  ReviewSubject::files_folder ── RepoName      ReviewSubject::files_folder ─┐
  Codebase::GitHub(RepoName) ─── RepoName      Codebase::Hosted ────────────┼─▶ HostedRepo::files_folder
  folders_opened_from_any_repo                 folders_opened_from_any_repo ┘   HostedRepo::of_files_folder
    └─ owner/name dirs, read as GitHub         Config clones, merge_methods ──▶ HostedRepo text form
  Config clones, merge_methods ── RepoName
```

## Requests for changes are a capability

Issue #140, 2026-11-25. `ReviewVerdict::RequestChanges` assumed every host has it. GitHub, Bitbucket and Gitea do; GitLab added a request-changes state to its reviews only in recent versions, and a self-managed GitLab may be older (not checked). So it is a capability, `ChangeRequestsPort::request_changes`, handed out by `CodeHost::change_requests`, and not core. `ReviewWritesPort` keeps what every host does: `submit_review` (no verdict) and `approve` (GitLab's approval is a call of its own; a GitLab adapter's `approve` would publish the draft notes and then approve, not checked). `ReviewVerdict` is the reviewer's choice in the finish form and lives in `finish_review`; `FinishRequest::run` turns it into the port call. `CommentDesk::for_host` holds the host's capabilities, so `Review::can_request_changes` is false and `finish_review_request` refuses with `FinishRefusal::NotOffered` on a host without it, before anything is sent; the finish form shows only Comment and Approve there (`Finishing::CommentOrApprove`). GitHub's event names (`COMMENT`, `APPROVE`, `REQUEST_CHANGES`) are the adapter's private `ReviewEvent`.

## Reactions on comments are a capability

Issue #140, 2026-11-25. `ReviewThreadsPort::set_reaction` made every host react to comments. It is `CommentReactionsPort::set_comment_reaction` now, handed out by `CodeHost::comment_reactions`. On a host without it a hand-off puts no seen mark (👀) and is delivered all the same (`SeenMark::Off`, no trouble named), the thread shows no reactions to add (`Review::can_react_to_comments`, from the `CommentDesk`'s capabilities), and a reaction asked anyway is refused with `CommentRefusal::NotOffered` before the host is asked. `DeliveryRequest`, `AgentReplyRequest`, `SeenMarkOff` and `ThreadActionRequest` run with a `&dyn CodeHost`, so each reaches the port through its `Option`. GitLab has award emoji on notes, so its adapter would offer it (not checked).

## How a PR would merge is the merging port's

Issue #140, 2026-11-25. `OpenPrStatus` held `Merging` (merge state, auto-merge, merge methods, what the viewer may do), so every host's status had to fill it. It is `MergingPort::pull_request_merging` now: the status job reads the status, and only for an open PR on a host with a merging port, how it would merge (`Done::PrStatus { status, merging }`). `PrControls` keeps it apart from the status (`merging_state`), only when the review offers merging, and a rebuild keeps it only then too; a rebuild that newly offers merging shows it after its next read. GitHub reads the two in two queries now, one more request per status read (every 30 s while checks run) on GitHub. Would GitLab fill `Merging`: `detailed_merge_status` for whether it merges now, `merge_when_pipeline_succeeds` for auto-merge, the project's merge method and squash option for the methods, and the user's merge permission (not checked). Approvals are not in it; they would be a capability of their own.

## Left open

- Tests of a local review other than `local_review.rs` and `local_guided_tour.rs` that go through `Review::open_basic` with `every_capability()` (for example `basic_tour.rs`, `move_to_pr.rs`) build a local review with a PR host's capabilities; they test reading, not capabilities.

