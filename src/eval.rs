use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hex;
use crate::pull_request::PullRequestUrl;
use crate::tour::Tour;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rule {
    Order,
    Grouping,
    Diagram,
    Text,
    Skim,
}

impl Rule {
    pub const ALL: [Self; 5] = [
        Self::Order,
        Self::Grouping,
        Self::Diagram,
        Self::Text,
        Self::Skim,
    ];
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Order => "order",
            Self::Grouping => "grouping",
            Self::Diagram => "diagram",
            Self::Text => "text",
            Self::Skim => "skim",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Score {
    Pass,
    Partial,
    Fail,
}

impl fmt::Display for Score {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pass => "pass",
            Self::Partial => "partial",
            Self::Fail => "fail",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub score: Score,
    pub reason: String,
}

/// One verdict for each of the five rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BTreeMap<Rule, Verdict>")]
pub struct Verdicts(BTreeMap<Rule, Verdict>);

impl TryFrom<BTreeMap<Rule, Verdict>> for Verdicts {
    type Error = String;

    fn try_from(verdicts: BTreeMap<Rule, Verdict>) -> Result<Self, String> {
        match Rule::ALL.iter().find(|rule| !verdicts.contains_key(rule)) {
            Some(missing) => Err(format!("no verdict for the {missing} rule")),
            None => Ok(Self(verdicts)),
        }
    }
}

impl Verdicts {
    pub fn from_pairs(pairs: impl IntoIterator<Item = (Rule, Verdict)>) -> Self {
        Self(pairs.into_iter().collect())
    }

    pub fn get(&self, rule: Rule) -> Option<&Verdict> {
        self.0.get(&rule)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Rule, &Verdict)> {
        self.0.iter().map(|(rule, verdict)| (*rule, verdict))
    }
}

/// The skill text under test. Its version is a hash, so an edit always shows as a new version.
pub struct Skill {
    text: String,
}

impl Skill {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
        }
    }

    pub fn bundled() -> Self {
        Self::new(include_str!("../skills/tour/SKILL.md"))
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn version(&self) -> String {
        let digest = Sha256::digest(self.text.as_bytes());
        hex::lower(digest.iter().take(4))
    }
}

/// All the tour writer gets. The reviewer note is deliberately not part of it.
#[derive(Debug)]
pub struct WriterBrief<'a> {
    pub pr: &'a PullRequestUrl,
    pub skill: &'a str,
    /// The only tours folder the writer may use; never the reviewer's own.
    pub tours_dir: &'a Path,
}

pub struct JudgeInput<'a> {
    pub note: &'a str,
    pub tour_json: &'a str,
}

/// Writes the tour of `brief.pr` into `brief.tours_dir`.
pub trait EvalWriter {
    fn write_tour(&self, brief: &WriterBrief) -> anyhow::Result<()>;
}

pub trait TourJudge {
    fn judge(&self, input: &JudgeInput) -> anyhow::Result<Verdicts>;
}

pub enum TourSource {
    WriteNew,
    ExistingTours,
}

/// `~/.guided-review/evals/`: reviewer notes (the test set) and run results. Private, never in the repo.
pub struct EvalStore {
    root: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunRecord {
    pub date: String,
    pub skill_version: String,
    pub prs: BTreeMap<String, Verdicts>,
}

struct TestPr {
    pr: PullRequestUrl,
    note: String,
}

impl EvalStore {
    pub fn in_home(home: &Path) -> Self {
        Self {
            root: home.join(".guided-review/evals"),
        }
    }

    pub fn add(&self, pr: &PullRequestUrl, note: &str) -> anyhow::Result<()> {
        let path = self.note_path(pr);
        let folder = path
            .parent()
            .with_context(|| format!("{} has no folder to write into", path.display()))?;
        std::fs::create_dir_all(folder)?;
        std::fs::write(&path, format!("{}\n", note.trim()))
            .with_context(|| format!("could not write {}", path.display()))
    }

    fn note_path(&self, pr: &PullRequestUrl) -> PathBuf {
        self.root
            .join("notes")
            .join(pr.repo.to_string())
            .join(format!("{}.md", pr.number))
    }

    fn test_set(&self) -> anyhow::Result<Vec<TestPr>> {
        let mut test_set = Vec::new();
        for owner in sorted_entries(&self.root.join("notes"))? {
            for repo in sorted_entries(&owner)? {
                for note in sorted_entries(&repo)? {
                    let owner = owner.file_name().unwrap_or_default().to_string_lossy();
                    let name = repo.file_name().unwrap_or_default().to_string_lossy();
                    let number = note.file_stem().unwrap_or_default().to_string_lossy();
                    test_set.push(TestPr {
                        pr: format!("https://github.com/{owner}/{name}/pull/{number}").parse()?,
                        note: std::fs::read_to_string(&note)?,
                    });
                }
            }
        }
        Ok(test_set)
    }

    fn last_run(&self) -> anyhow::Result<Option<RunRecord>> {
        let Some(latest) = sorted_entries(&self.root.join("results"))?
            .into_iter()
            .rfind(|path| path.extension().is_some_and(|ext| ext == "json"))
        else {
            return Ok(None);
        };
        let text = std::fs::read_to_string(&latest)?;
        Ok(Some(serde_json::from_str(&text).with_context(|| {
            format!("{} is not a valid eval result", latest.display())
        })?))
    }

    fn save_run(&self, run: &RunRecord) -> anyhow::Result<()> {
        let folder = self.root.join("results");
        std::fs::create_dir_all(&folder)?;
        let path = folder.join(format!("{}.json", run.date));
        std::fs::write(&path, serde_json::to_string_pretty(run)? + "\n")
            .with_context(|| format!("could not write {}", path.display()))
    }

    fn run_tours_dir(&self, date: &str) -> PathBuf {
        self.root.join("tours").join(date)
    }
}

/// A missing folder lists as empty.
fn sorted_entries(folder: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Ok(Vec::new());
    };
    let mut paths: Vec<PathBuf> = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    paths.sort();
    Ok(paths)
}

pub struct Eval<'a> {
    pub store: &'a EvalStore,
    pub home: &'a Path,
    pub writer: &'a dyn EvalWriter,
    pub judge: &'a dyn TourJudge,
    pub skill: &'a Skill,
}

impl Eval<'_> {
    pub fn run(&self, source: &TourSource, date: &str) -> anyhow::Result<Report> {
        let test_set = self.store.test_set()?;
        if test_set.is_empty() {
            bail!(
                "the test set is empty; add a PR with `guided-review eval add <pr-url> --note <text>`"
            );
        }
        let previous = self.store.last_run()?;
        let mut prs = BTreeMap::new();
        let mut failures = Vec::new();
        for TestPr { pr, note } in &test_set {
            match self.score(pr, note, source, date) {
                Ok(verdicts) => {
                    prs.insert(pr.to_string(), verdicts);
                }
                Err(error) => failures.push((pr.to_string(), format!("{error:#}"))),
            }
        }
        let record = RunRecord {
            date: date.to_owned(),
            skill_version: self.skill.version(),
            prs,
        };
        if !record.prs.is_empty() {
            self.store.save_run(&record)?;
        }
        Ok(Report {
            record,
            previous,
            failures,
        })
    }

    fn score(
        &self,
        pr: &PullRequestUrl,
        note: &str,
        source: &TourSource,
        date: &str,
    ) -> anyhow::Result<Verdicts> {
        match source {
            TourSource::ExistingTours => self.judge_tour(&Tour::path_for(self.home, pr), note),
            TourSource::WriteNew => {
                let tours_dir = self.store.run_tours_dir(date);
                self.writer.write_tour(&WriterBrief {
                    pr,
                    skill: self.skill.text(),
                    tours_dir: &tours_dir,
                })?;
                let tour_path = Tour::path_in(&tours_dir, pr);
                if !tour_path.exists() {
                    bail!("the writer did not leave a tour file");
                }
                self.judge_tour(&tour_path, note)
            }
        }
    }

    fn judge_tour(&self, tour_path: &Path, note: &str) -> anyhow::Result<Verdicts> {
        let tour_json = std::fs::read_to_string(tour_path)
            .with_context(|| format!("no tour at {}", tour_path.display()))?;
        self.judge.judge(&JudgeInput {
            note,
            tour_json: &tour_json,
        })
    }
}

pub struct Report {
    record: RunRecord,
    previous: Option<RunRecord>,
    failures: Vec<(String, String)>,
}

impl Report {
    pub fn has_failures(&self) -> bool {
        !self.failures.is_empty()
    }
}

fn short_pr(url: &str) -> String {
    url.parse::<PullRequestUrl>()
        .map_or_else(|_| url.to_owned(), |pr| pr.short())
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "eval {} - skill version {}",
            self.record.date, self.record.skill_version
        )?;
        for (pr, verdicts) in &self.record.prs {
            writeln!(f, "{}", short_pr(pr))?;
            for (rule, verdict) in verdicts.iter() {
                writeln!(f, "  {rule}: {} - {}", verdict.score, verdict.reason)?;
            }
        }
        for (pr, reason) in &self.failures {
            writeln!(f, "{}\n  failed: {reason}", short_pr(pr))?;
        }
        let Some(previous) = &self.previous else {
            return writeln!(f, "first run, nothing to compare with");
        };
        writeln!(
            f,
            "since {} (skill version {})",
            previous.date, previous.skill_version
        )?;
        let mut changes = 0;
        for (pr, verdicts) in &self.record.prs {
            let Some(before) = previous.prs.get(pr) else {
                continue;
            };
            for (rule, now) in verdicts.iter() {
                let Some(was) = before.get(rule) else {
                    continue;
                };
                if was.score != now.score {
                    changes += 1;
                    writeln!(
                        f,
                        "  {} {rule}: {} -> {} - {}",
                        short_pr(pr),
                        was.score,
                        now.score,
                        now.reason
                    )?;
                }
            }
        }
        if changes == 0 {
            writeln!(f, "  no changes")?;
        }
        Ok(())
    }
}
