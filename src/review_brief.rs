//! `guided-review review-brief`: what the review agent is told when it starts, printed by the CLI so
//! the first task stays one line and the rules live beside the code that enforces them.

use std::fmt;
use std::path::PathBuf;

use crate::app_folders::AppFolders;
use crate::instructions::ReviewInstructions;
use crate::review_subject::ReviewSubject;
use crate::tour::Tour;

pub struct ReviewBrief {
    subject: ReviewSubject,
    findings_path: PathBuf,
    instructions: ReviewInstructions,
}

impl ReviewBrief {
    pub fn of(folders: &AppFolders, subject: &ReviewSubject) -> Self {
        Self {
            subject: subject.clone(),
            findings_path: Tour::findings_path_for(folders, subject),
            instructions: ReviewInstructions::of(folders),
        }
    }

    /// A launch warning, such as a config that could not be read, for stderr.
    pub fn warning(&self) -> Option<&str> {
        self.instructions.warning()
    }
}

impl fmt::Display for ReviewBrief {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let subject = &self.subject;
        let path = self.findings_path.display();
        write!(
            f,
            "\
You are reviewing {subject} for the reviewer, who reads your findings in the app. Never change code,
never post on GitHub, never push. Your only output is the findings file.

Run one command per Bash call: no `&&`, `;`, pipes or `$(...)`.

1. Read the change: `guided-review outline {subject}`, then `guided-review show {subject} <piece ids> --around`.
   Read the code around it in your working folder, which holds the reviewed head, and run the tests
   when that helps to settle a doubt.
2. Write the findings file with the Write tool, whole, at {path}
   It is JSON: {{\"findings\": [{{\"id\": \"<short unique id>\", \"severity\": \"must fix\" | \"should fix\" | \"nit\",
   \"body\": \"<markdown, first line is the title>\", \"anchor\": <anchor>}}]}}
   An anchor is {{\"kind\": \"change\"}}, {{\"kind\": \"file\", \"file\": \"<path>\"}} or
   {{\"kind\": \"lines\", \"file\": \"<path>\", \"side\": \"base\" | \"head\", \"lines\": [<from>, <to>]}},
   on lines the diff shows. Write a new file even when one is there: it replaces the earlier run's.
3. Run `guided-review check-findings {subject}` and fix what it lists, until it says 0 errors.
4. Run `guided-review findings-done {subject}`. Without it the review counts as failed.
5. Stop.

dismissed findings: none
"
        )?;
        write!(f, "{}", self.instructions)
    }
}
