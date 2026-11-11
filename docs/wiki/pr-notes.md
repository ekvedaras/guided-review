# PR notes

2026-11-11. The reviewer can put a step's diagram, a picture of the reviewer summary, or the whole summary under `Notes` in the PR description, each folded in a `<details>`. Code: `src/viewer/pr_notes.rs`.

## Pictures go up through GitHub's drag-and-drop store

A summary picture is a file on the reviewer's machine, so GitHub must hold a copy before the description can show it. `GitHub::upload_picture` posts it to `uploads.github.com/user-attachments/assets`, the store the web page uses for drag and drop. A link to it opens only for people who can see the repository, as a picture dropped on the page does.

Rejected: committing the pictures to the PR's branch, which puts them in the diff under review; a public image host, which leaks a private PR's pictures.

Known risk: the store is not in GitHub's documented API. It takes the token `gh` holds, and it answers 404 when that token cannot push to the repository. If GitHub changes it, uploads fail with GitHub's answer beside the button, and diagrams still work, as they need no upload.

## How the app knows a note is there already

A diagram is found by its own text, which also finds diagrams added before the fold existed. A picture's address is new on every upload, so a picture or summary carries a hidden comment, `<!-- guided-review picture <digest> -->` or `<!-- guided-review summary <digest> -->`. The digest covers the picture's bytes, or the summary's text and all of its pictures, so a redrawn picture or an edited summary can be added again.

A `#` line inside a `<details>` fold belongs to the fold, so a folded summary's headings do not end the `Notes` section.
