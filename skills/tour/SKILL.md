---
name: tour
description: Write a top-down guided-review tour of a GitHub PR with the guided-review CLI. Use when the user types /tour <pr-url> or asks for a guided review of a PR.
---

# /tour <pr-url>

Write a **tour file**: the PR's changes as **steps**, main flow first, details later. The `guided-review` CLI owns the facts (piece ids, shas); you own the story: order, nesting, text, diagrams, skim flags, summary. Terms: `GLOSSARY.md` in the guided-review repo.

The tour is done when `guided-review check <pr-url>` prints `0 errors` and you have handled every warning.

## Steps

1. Intent: `gh pr view <pr-url> --json title,body`.
2. Outline: `guided-review outline <pr-url>`. It is the plan; start from it, not from the full diff.
   - `small PR` hint: read the whole diff with `guided-review show <pr-url> <every piece id>` and skip step 3.
   - `worktree at head: yes`: you may grep and read the working tree. With `no`, the working tree is another version of the code; read code only through `show --around`.
   - A test piece lists `subjects`, best first. Put a test next to its subject, or in a late proof step.
3. Open only the pieces you need to understand the flow: `guided-review show <pr-url> <piece id>…`. A file path as id opens every piece of that file. Add `--around` when the change needs the code near it at head. Pieces you can place from signature and size alone stay closed.
4. Write the tour to `~/.guided-review/tours/<owner>/<repo>/<pr-number>.json` (`mkdir -p` first), in the shape below. Leave `version`, `pr`, `base`, `head` out; `check` fills them.
5. Run `guided-review check <pr-url>`. Fix every error. Handle every warning by changing the tour:
   - `no step covers <piece>`: put the piece in the step it belongs to, or in a `rest` piece under a `skim` step. Noise (renames, imports, generated code) goes to that `skim` step.
   - A limit warning (summary, bullets, `why`, pieces per step, top-level steps): shorten, split, or nest as it says. Pieces per step counts every piece the step names, imports and `(top)` included, but not what `rest` takes.
   Run `check` again. Repeat until `0 errors, 0 warnings`.

## Order

- First step: the new contract or entry point a reviewer must hold in mind to understand the rest.
- Then follow the call flow downward. Children are the details of their parent: gist first, how next, detail last.
- Callers that only pass a flag or opt out go late, in one `skim` step.
- Tests come last as proof, unless a test is the clearest spec of the behaviour.
- At most 7 top-level steps; nest the rest.

## Text

A reviewer skims: short lines, visuals over prose. Text fields are small Markdown (inline code, bold).

- `summary`: at most 3 bullets, about 12 words each.
- `why`: one or two lines. What this is and why it comes here.
- `check`: risky steps only. One line saying what to verify.
- `tldr`: only when a step has several separate points.
- `diagram`: Mermaid source on a step that holds a state machine, a flow across classes, or a tricky sequence. Under 12 nodes.

## Shape

```json
{
  "title": "Short name of the change",
  "summary": ["Max three bullets"],
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
