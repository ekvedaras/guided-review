# Glossary

**Tour**: A guided review of one PR. It shows the changes in top-down order: main flow first, details later.

**Summary**: The short text at the start of a tour. At most three bullets that say what the PR does.

**Step**: One stop in a tour. It holds diff pieces, a diagram, or both, and a short text that says what they do and why they come at this point. A step can have child steps: the details of that step. The reader sees a step first, then its child steps.

**Check hint**: A one-line note on a risky step that tells the reviewer what to verify.

**Skim step**: A step the reviewer only needs to glance at, for example callers that only pass a flag.

**Diff piece**: A part of a PR diff that a step can point to. It can be smaller than a file, even part of one method.

**Tour file**: The saved form of a tour. Any viewer can show it.

**Viewer**: A thing that shows a tour file to the reviewer and tracks which steps were read.
