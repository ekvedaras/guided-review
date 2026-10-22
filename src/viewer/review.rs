use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::code_source::CodeSource;
use crate::coverage;
use crate::pieces::{self, ChangedFile};
use crate::pull_request::{PullRequestUrl, Sha};
use crate::tour::{Step, Tour};

use std::path::PathBuf;

use crate::config::AskSettings;

use super::ask::Place;
use super::ask_context;
use super::github_link::GitHubLinks;
use super::highlight::{HighlightRequest, Highlights};
use super::ports::{AgentLaunch, GitHubPort};
use super::review_state::{ReviewState, ReviewStateFile};
use super::step_page::{SafetyNetPage, StepPage};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StepId(String);

impl From<&str> for StepId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl fmt::Display for StepId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Summary,
    Step(StepId),
    /// After the last step: every change no step shows.
    SafetyNet,
}

/// One row of the step tree. Rows come in reading order, so indenting by `depth` draws the tree.
pub struct SidebarEntry<'a> {
    pub id: StepId,
    pub title: &'a str,
    pub depth: usize,
    pub is_current: bool,
    pub is_ticked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadProgress {
    pub read: usize,
    pub total: usize,
}

/// The tour was written for an older head than the PR has now, so it may be stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OldHead {
    pub tour_head: Sha,
    pub pr_head: Sha,
}

pub struct Review {
    tour: Tour,
    pr: PullRequestUrl,
    base: Sha,
    head: Sha,
    /// Unknown until GitHub answers.
    pr_head: Option<Sha>,
    files: Vec<ChangedFile>,
    /// Summary, then parent, its children, the next parent (from the big picture to the details),
    /// then the safety net when it holds anything.
    reading_order: Vec<Screen>,
    screen: Screen,
    state: ReviewState,
    state_file: ReviewStateFile,
    /// Skim and ticked steps start folded; these are the steps whose fold the reviewer flipped.
    /// Not saved: a reopened app shows steps folded again.
    fold_flipped: BTreeSet<StepId>,
    highlights: Highlights,
}

impl Review {
    /// Code is read at the tour's own base and head, not the PR's current ones.
    pub fn open(
        tour: Tour,
        source: &dyn CodeSource,
        state_file: ReviewStateFile,
    ) -> anyhow::Result<Self> {
        let state = state_file.load()?;
        let pr: PullRequestUrl = tour.pr.parse()?;
        let (base, head) = (Sha::new(&tour.base), Sha::new(&tour.head));
        let files = pieces::cut(source, &base, &head)?;
        let steps = tour
            .steps()
            .into_iter()
            .map(|step| Screen::Step(StepId::from(step.id.as_str())));
        let safety_net =
            (!coverage::safety_net(&tour, &files).is_empty()).then_some(Screen::SafetyNet);
        let reading_order = std::iter::once(Screen::Summary)
            .chain(steps)
            .chain(safety_net)
            .collect();
        Ok(Self {
            tour,
            pr,
            base,
            head,
            pr_head: None,
            files,
            reading_order,
            screen: Screen::Summary,
            state,
            state_file,
            fold_flipped: BTreeSet::new(),
            highlights: Highlights::default(),
        })
    }

    /// Colouring reads whole files, so the app runs this off the main thread and hands the result to `set_highlights`.
    pub fn highlight_request(&self) -> HighlightRequest {
        HighlightRequest::for_files(
            &self.files,
            &Sha::new(&self.tour.base),
            &Sha::new(&self.tour.head),
        )
    }

    pub fn set_highlights(&mut self, highlights: Highlights) {
        self.highlights = highlights;
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn pr(&self) -> &PullRequestUrl {
        &self.pr
    }

    /// Blocking; the app runs it in the background on open and on focus.
    pub fn check_pr_head(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        let tips = github.pull_request_tips(&self.pr)?;
        self.pr_head_is(tips.head);
        Ok(())
    }

    pub fn pr_head_is(&mut self, head: Sha) {
        self.pr_head = Some(head);
    }

    pub fn old_head(&self) -> Option<OldHead> {
        self.pr_head
            .as_ref()
            .filter(|pr_head| **pr_head != self.head)
            .map(|pr_head| OldHead {
                tour_head: self.head.clone(),
                pr_head: pr_head.clone(),
            })
    }

    pub fn title(&self) -> &str {
        &self.tour.title
    }

    pub fn summary(&self) -> &[String] {
        &self.tour.summary
    }

    pub fn current_step(&self) -> Option<StepPage<'_>> {
        let Screen::Step(id) = &self.screen else {
            return None;
        };
        let step = self.tour.steps().into_iter().find(|step| step.id == id.0)?;
        let mut page = StepPage::of(
            step,
            &self.tour,
            &self.files,
            &self.links(),
            &self.highlights,
            self.is_folded(id),
        );
        page.is_ticked = self.is_ticked(id);
        Some(page)
    }

    pub fn head(&self) -> &str {
        &self.tour.head
    }

    /// What the AI is told about a place: pointers, the shown diff lines, step title and why.
    pub fn ask_context(&self, place: &Place) -> Option<String> {
        ask_context::context_for(&self.tour, &self.files, &self.links(), place)
    }

    pub fn agent_launch(
        &self,
        cwd: PathBuf,
        settings: &AskSettings,
        resume: Option<String>,
    ) -> AgentLaunch {
        AgentLaunch {
            cwd,
            model: settings.model.clone(),
            effort: settings.effort.clone(),
            system_prompt: ask_context::system_prompt(&self.tour.pr),
            resume,
        }
    }

    pub fn sidebar(&self) -> Vec<SidebarEntry<'_>> {
        fn walk<'a>(
            steps: &'a [Step],
            depth: usize,
            current: Option<&StepId>,
            state: &ReviewState,
            into: &mut Vec<SidebarEntry<'a>>,
        ) {
            for step in steps {
                let id = StepId::from(step.id.as_str());
                into.push(SidebarEntry {
                    is_current: current == Some(&id),
                    is_ticked: state.read_tick(&id).is_some(),
                    id,
                    title: &step.title,
                    depth,
                });
                walk(&step.children, depth + 1, current, state, into);
            }
        }
        let current = match &self.screen {
            Screen::Step(id) => Some(id),
            Screen::Summary | Screen::SafetyNet => None,
        };
        let mut entries = Vec::new();
        walk(&self.tour.steps, 0, current, &self.state, &mut entries);
        entries
    }

    /// `None` when every change is in a step.
    pub fn safety_net(&self) -> Option<SafetyNetPage> {
        self.reading_order
            .contains(&Screen::SafetyNet)
            .then(|| SafetyNetPage::of(&self.tour, &self.files, &self.links(), &self.highlights))
    }

    fn links(&self) -> GitHubLinks<'_> {
        GitHubLinks {
            pr: &self.pr,
            base: &self.base,
            head: &self.head,
        }
    }

    /// Ids the tour does not hold are ignored: the sidebar only offers real steps.
    pub fn go_to(&mut self, id: &StepId) {
        self.go_to_screen(Screen::Step(id.clone()));
    }

    pub fn go_to_summary(&mut self) {
        self.screen = Screen::Summary;
    }

    pub fn go_to_safety_net(&mut self) {
        self.go_to_screen(Screen::SafetyNet);
    }

    fn go_to_screen(&mut self, screen: Screen) {
        if self.reading_order.contains(&screen) {
            self.screen = screen;
        }
    }

    /// False when there is no screen after this one.
    pub fn go_forward(&mut self) -> bool {
        self.step_by(1)
    }

    /// False on the summary, the first screen.
    pub fn go_back(&mut self) -> bool {
        self.step_by(-1)
    }

    pub fn is_ticked(&self, id: &StepId) -> bool {
        self.state.read_tick(id).is_some()
    }

    /// The tick is kept in memory even when saving fails, so the reviewer loses nothing this session.
    pub fn tick(&mut self, id: &StepId) -> anyhow::Result<()> {
        if !self.reading_order.contains(&Screen::Step(id.clone())) {
            return Ok(());
        }
        let head = Sha::new(&self.tour.head);
        self.state.tick(id, &head);
        self.fold_flipped.remove(id);
        self.state_file.update(|saved| saved.tick(id, &head))
    }

    pub fn untick(&mut self, id: &StepId) -> anyhow::Result<()> {
        self.state.untick(id);
        self.fold_flipped.remove(id);
        self.state_file.update(|saved| saved.untick(id))
    }

    /// Skim and ticked steps fold; the reviewer can flip that for the session without losing the tick.
    pub fn is_folded(&self, id: &StepId) -> bool {
        let folds_by_default = self.is_ticked(id) || self.is_skim(id);
        folds_by_default != self.fold_flipped.contains(id)
    }

    pub fn toggle_fold(&mut self, id: &StepId) {
        if !self.fold_flipped.remove(id) {
            self.fold_flipped.insert(id.clone());
        }
    }

    fn is_skim(&self, id: &StepId) -> bool {
        self.tour
            .steps()
            .into_iter()
            .any(|step| step.id == id.0 && step.skim)
    }

    /// Only steps of this tour count: ticks of steps a refresh removed stay in the file but not here.
    pub fn progress(&self) -> ReadProgress {
        let steps = self.reading_order.iter().filter_map(|screen| match screen {
            Screen::Step(id) => Some(id),
            Screen::Summary | Screen::SafetyNet => None,
        });
        ReadProgress {
            read: steps.clone().filter(|id| self.is_ticked(id)).count(),
            total: steps.count(),
        }
    }

    fn step_by(&mut self, offset: isize) -> bool {
        let next = self
            .reading_order
            .iter()
            .position(|screen| screen == &self.screen)
            .and_then(|position| position.checked_add_signed(offset))
            .and_then(|position| self.reading_order.get(position));
        match next {
            Some(screen) => {
                self.screen = screen.clone();
                true
            }
            None => false,
        }
    }
}
