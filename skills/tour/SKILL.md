---
name: tour
description: Write a top-down guided-review tour of a GitHub PR with the guided-review CLI. Use when the user types /tour <pr-url> or asks for a guided review of a PR.
---

# /tour <pr-url> [--redo]

Write a **tour file**: the PR's changes as **steps**, main flow first, details later. Run each command as its own Bash call, with no `&&`, `;`, pipes or shell variables. The `guided-review` CLI owns the facts (piece ids, shas); you own the story: order, nesting, text, diagrams, skim flags, summary. Terms: `GLOSSARY.md` in the guided-review repo.

The tour is done when `guided-review check <pr-url>` prints `0 errors` and you have handled every warning. Then **open** it: run `guided-review open <pr-url>`. It hands the PR to the one running app (starting it when none runs) and returns at once with a line saying what the app did. Reply with the tour summary at once.

## Refresh and redo

- The tour file (path: `guided-review tour-path <pr-url>`) exists and no `--redo`: this is a **Refresh**, follow "Refresh" at the end. Do not write a new tour.
- No tour file, or `--redo`: write a full new tour with the steps below, replacing the old file.

## Steps

1. Intent: `gh pr view <pr-url> --json title,body`.
2. Outline: `guided-review outline <pr-url>`. It is the plan; start from it, not from the full diff.
   - `small PR` hint: read the whole diff with `guided-review show <pr-url> <every piece id>` and skip step 3.
   - A `summary instructions: run the skill <name>` line at the end means: call the Skill tool with that name, follow what it says, and write the result as Markdown in `reviewer_summary`. Otherwise a `summary instructions:` block at the end holds the reviewer's own instructions for an extra summary. Follow its text and write the result as Markdown in the tour's `reviewer_summary` field, next to `summary`. It has no length limit, and `summary` stays at most 3 bullets. Without the block, leave `reviewer_summary` out. A `warning:` line there means the instructions file could not be read: write no `reviewer_summary` and mention the warning in your reply.
   - `worktree at head: yes`: you may grep and read the working tree. With `no`, the working tree is another version of the code; read code only through `show --around`.
   - A test piece lists up to 3 `subjects`, best first (`+N more` hides the rest; `same` repeats the line above). Put a test next to its subject, or in a late proof step.
3. Open only the pieces you need to understand the flow: `guided-review show <pr-url> <piece id>…`. A file path as id opens every piece of that file. Add `--around` when the change needs the code near it at head. Pieces you can place from signature and size alone stay closed.
4. Write the tour to the path `guided-review tour-path <pr-url>` prints with the Write tool, which makes the folder (no `mkdir`: an app-started writer may run only the commands this skill names), in the shape below. Leave `version`, `pr`, `base`, `head` out; `check` fills them.
5. Run `guided-review check <pr-url>`. Fix every error. Handle every warning by changing the tour:
   - `no step covers <piece>`: put the piece in the step it belongs to, or in a `rest` piece under a `skim` step. Noise (renames, imports, generated code) goes to that `skim` step.
   - A limit warning (summary, bullets, `why`, pieces per step, top-level steps): shorten, split, or nest as it says. Pieces per step counts every piece the step names, imports and `(top)` included, but not what `rest` takes.
   `check` also prints a `note:` per `rest` pointer with what it took; confirm it is only noise. Run `check` again. Repeat until `0 errors, 0 warnings`.

## Order

- First step: the new contract or entry point a reviewer must hold in mind to understand the rest.
- Then follow the call flow downward. Children are the details of their parent: gist first, how next, detail last.
- Callers that only pass a flag or opt out go late, in one `skim` step.
- Tests come last as proof, unless a test is the clearest spec of the behaviour.
- At most 7 top-level steps; nest the rest.

## Text

A reviewer skims: short lines, visuals over prose. Text fields are small Markdown (inline code, bold).

- `summary`: at most 3 bullets, about 12 words each.
- `reviewer_summary`: only with summary instructions. Markdown, as long as they ask for. Pictures and pages it shows go in the tour's own folder: the tour file's path without `.json` (`guided-review tour-path <pr>` prints it). Name them by a relative path: `![deadline](deadline.png)` shows the picture in the app, and `[Open the explainer](explainer.html)` opens the page in the browser. An image on GitHub is shown only as a link.
- `why`: one or two lines. What this is and why it comes here.
- `check`: risky steps only. One line saying what to verify.
- `tldr`: only when a step has several separate points.
- `diagram`: Mermaid source on a step that holds a state machine, a flow across classes, or a tricky sequence. Under 12 nodes.

## Shape

```json
{
  "title": "Short name of the change",
  "summary": ["Max three bullets"],
  "reviewer_summary": "Optional Markdown, only with summary instructions",
  "steps": [
    {
      "id": "kebab-id",
      "title": "What the reader learns here",
      "why": "One or two lines.",
      "check": "Optional.",
      "tldr": ["Optional"],
      "diagram": "Optional Mermaid",
      "skim": false,
      "pieces": [
        {"file": "app/Foo.php", "symbol": "Foo::handle"},
        {"file": "app/Bar.php"},
        {"file": "weird.xml", "lines": [10, 40]},
        {"rest": "app/Services"}
      ],
      "children": []
    }
  ]
}
```

- `symbol` is the part of the outline id after `#`: id `app/Foo.php#Foo::handle` is `{"file": "app/Foo.php", "symbol": "Foo::handle"}`.
- A piece with only `file` takes every piece of that file. `lines` is for files the outline shows as hunks.
- `{"rest": "dir"}` takes every piece under `dir` that no other pointer covers; `""` takes everything left. Use it for skim steps.
- A step may have no pieces when it holds only a diagram or a `tldr`.

## Refresh

Rewrite only what the new commits made stale; every other step stays as it is, word for word. `summary` and `reviewer_summary` stay word for word too; only a redo writes the reviewer summary again.

1. `guided-review repin <pr-url>` moves the tour to the PR's current head (a no-op when it is there already).
2. `guided-review refresh <pr-url>` prints the brief: `stale steps` (their text may be old) and `new pieces no step holds`. `nothing to refresh` means stop and say so.
3. Read the tour file, `guided-review outline <pr-url>` and `guided-review show` for the stale steps' pieces and the new pieces only.
4. Edit the tour file:
   - Each stale step: rewrite its `why`, `check`, `tldr` and `diagram` so they are true of the new code. Keep its `id`, place and pieces.
   - Each new piece: add a pointer to the step it belongs to. Noise goes to the `skim` step. Add a new step only when no step fits.
   - Leave all other steps untouched.
5. `guided-review check <pr-url>` until `0 errors, 0 warnings`.
6. `guided-review refresh <pr-url> --done` clears the marks, then `guided-review open <pr-url>` as above.
7. If the brief has a `redo:` line, tell the user a full `/tour --redo <pr-url>` may read better. Never start a redo yourself.

## Reviewer questions

The app types a reviewer's comment into this session when they send it with Ask agent. The message starts `Reviewer comment h0 on <pr-url>` and gives the place, the step, the thread so far and the comment.

1. Answer from the code and the tour: `guided-review show <pr-url> <ids>` for the pieces involved.
2. Post the answer: `guided-review agent-reply <pr-url> h0 --body "<answer>"`. It goes to the comment's thread on GitHub under the reviewer's account, so it starts with "🤖 " (added for you, once). It joins the reviewer's pending review when they have one and is posted at once when not; it never sends their review. Write it for the PR's author to read too, since they see it. Answer every hand-off with exactly one `agent-reply --body`, even when the answer is that you cannot tell. The app put 👀 on the comment when it gave it to you; your reply takes it off.
3. When the comment asks for a change to the PR's code, first run `guided-review agent-reply <pr-url> h0 --working`. It posts "🤖 Working on it…" on the thread, and your `--body` answer later replaces that comment. Make the change in your folder when it is a clone of the PR's repo on the PR's branch with none of the reviewer's own changes in it; else in a new git worktree, or a shallow clone, of the PR's branch. Run the repo's checks, commit, push to the PR's branch, and name the commit in the answer.
4. When the comment shows the tour is wrong or unclear, fix the tour file first, run `guided-review check <pr-url>` until it is clean, then `guided-review open <pr-url>` (the app reloads the tour in place), then reply saying what you changed.
5. A refusal (an unknown id, a thread that is gone) is printed with its reason; do not retry with another id. Tell the user instead.
