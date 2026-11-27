# Agent threads and the Ask panel's removal

2026-11-27. The maintainer replaced the Ask panel with a private thread to the tour agent. The code is in the commits "Remove the Ask panel", "Rename the comment-and-hand-off action" and the one that adds Ask agent; this page records the decisions and what stays open.

## Decisions (maintainer, 2026-11-27)

- The Ask panel is gone: its `A` key, palette action, the `asks` state section, `ask_model` and `ask_effort` with their Settings section, the Ask scene, `ClaudeCli` and the agent ports only it used. Old state files keep an `asks` key as an unknown section (kept, never read). A `[keys]` line `ask_about_step` is now an unknown name, reported once like any other (open: it could be ignored silently).
- The action that posts to GitHub and hands the comment to the tour agent is **Comment and ask agent** (and **Comment and ask agent now**). The old config values (`primary_comment_action`, `secondary_comment_action` = `ask_agent`, `ask_agent_now`) and the `[keys]` entries of those names keep their old meaning (`SendAction::former_config_text`, `Command::former_config_name`). Settings writes the new names.
- **Ask agent** is the new action for the tour agent alone, in PR mode, on lines or a file (`CommentPlace`), sending now only: no task queue for a PR. The thread is kept in the state file and the tour agent answers into it with `agent-reply`.
- A thread's home is a property of the thread. A PR review holds GitHub's threads and agent threads side by side.
- It looks like a normal thread with a small "Not on GitHub" chip. Scene `100-agent-thread`.
- Ask why with no review agent alive goes to the tour agent (research Q5a).
- Move to PR of a local review's tasks is unchanged: open tasks still become draft comments for GitHub. Open question: carry them as agent threads instead.

## What the agent decided

- Config text of the new action is `ask_agent_only`, since `ask_agent` has to keep meaning Comment and ask agent. Default key `Cmd+Alt+P`; no other action stands in for it (a box without it falls back to Add to review, never to an action that posts).
- A thread's home is told by its id: the ids `LocalThreads` gives start with `local-`, no host's do (`ThreadId::is_private`). `ThreadHomes` (`local_thread_store.rs`) picks the home of a thread, of a comment and of a new thread (a task mode, `SendMode::SendNow`, is private; in a PR it is the only one). A PR's send modes are Add to review, Comment now and Send now.
- One read job (`PullThreads`) reads every home and merges; the outcome of a send or a thread action replaces only its own home's threads (`ReviewCore::set_threads_of`). Open: when the host does not answer, the whole read fails, so agent threads do not show until it does; threads already shown stay.
- The message the agent is typed for a private question starts `Private question hN on <pr>`, says `Not on GitHub`, carries the code, the thread so far and `Question:`, and answers like a PR comment except that nothing is posted and no 👀 goes on a comment (`HandOff::private_message`). `agent-reply` finds the home from the hand-off's thread, so the same command writes the answer to the state file and says so (`ReplyPlacement::KeptOnThread`).
- Ask why on a finding with no review agent: the question and the finding, quoted, go into a comment box on the finding's lines, which the session sends as an Ask agent question. A finding on the whole change has no lines, so the question is refused with a reason. The finding stays open.
- `SelectedLines` no longer hands out its rows; its three callers ask for head texts, the text told to an agent, or numbered texts. This cleared a field leak the removal had made.
- The deslop allow lists lost 6 entries (the Ask desk's) with the removal, the tangle key naming `comments_and_asks` was renamed to `comments_here`, and the agent thread work cut 8 more: `agent_reply` takes the homes as a closure (`ThreadHomes::agent_reply` wraps it), so `HandOffs` does not name `ThreadHomes`; the snapshot staging of local tasks left `LocalThreadStore` (`local_task_staging.rs`); the text box keeps its sending keys in one `SendKeys`. The `hand_offs` / `local_thread_store` / `review_state` tangle is smaller but still allowed: `AgentReplyRequest` holds a `ThreadHome`, which holds a store of the state file that holds the hand-offs.
