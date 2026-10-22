use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::code_source::CodeSource;
use crate::pieces::{self, ChangedFile};
use crate::pull_request::Sha;
use crate::tour::{Step, Tour};

use std::path::PathBuf;

use crate::config::AskSettings;

use super::ask::Place;
use super::ask_context;
use super::highlight::{HighlightRequest, Highlights};
use super::ports::AgentLaunch;
use super::review_state::{ReviewState, ReviewStateFile};
use super::step_page::StepPage;

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

pub struct Review {
    tour: Tour,
    files: Vec<ChangedFile>,
    /// Parent, its children, then the next parent: from the big picture to the details.
    reading_order: Vec<StepId>,
    screen: Screen,
    state: ReviewState,
    state_file: ReviewStateFile,
    /// Ticked steps the reviewer opened again to reread. Not saved: a reopened app shows them folded.
    unfolded: BTreeSet<StepId>,
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
        let files = pieces::cut(source, &Sha::new(&tour.base), &Sha::new(&tour.head))?;
        let reading_order = tour
            .steps()
            .iter()
            .map(|step| StepId::from(step.id.as_str()))
            .collect();
        Ok(Self {
            tour,
            files,
            reading_order,
            screen: Screen::Summary,
            state,
            state_file,
            unfolded: BTreeSet::new(),
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
        let mut page = StepPage::of(step, &self.tour, &self.files, &self.highlights);
        page.is_ticked = self.is_ticked(id);
        page.is_folded = self.is_folded(id);
        Some(page)
    }

    pub fn head(&self) -> &str {
        &self.tour.head
    }

    /// What the AI is told about a place: pointers, the shown diff lines, step title and why.
    pub fn ask_context(&self, place: &Place) -> Option<String> {
        ask_context::context_for(&self.tour, &self.files, place)
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
            Screen::Summary => None,
            Screen::Step(id) => Some(id),
        };
        let mut entries = Vec::new();
        walk(&self.tour.steps, 0, current, &self.state, &mut entries);
        entries
    }

    /// Ids the tour does not hold are ignored: the sidebar only offers real steps.
    pub fn go_to(&mut self, id: &StepId) {
        if self.reading_order.contains(id) {
            self.screen = Screen::Step(id.clone());
        }
    }

    pub fn go_to_summary(&mut self) {
        self.screen = Screen::Summary;
    }

    /// False when there is no step after this screen.
    pub fn go_forward(&mut self) -> bool {
        let next = match self.position() {
            None => 0,
            Some(position) => position + 1,
        };
        match self.reading_order.get(next) {
            Some(id) => {
                self.screen = Screen::Step(id.clone());
                true
            }
            None => false,
        }
    }

    /// False on the summary, the first screen.
    pub fn go_back(&mut self) -> bool {
        match self.position() {
            None => false,
            Some(0) => {
                self.screen = Screen::Summary;
                true
            }
            Some(position) => {
                self.screen = Screen::Step(self.reading_order[position - 1].clone());
                true
            }
        }
    }

    pub fn is_ticked(&self, id: &StepId) -> bool {
        self.state.read_tick(id).is_some()
    }

    /// The tick is kept in memory even when saving fails, so the reviewer loses nothing this session.
    pub fn tick(&mut self, id: &StepId) -> anyhow::Result<()> {
        if !self.reading_order.contains(id) {
            return Ok(());
        }
        let head = Sha::new(&self.tour.head);
        self.state.tick(id, &head);
        self.unfolded.remove(id);
        self.state_file.update(|saved| saved.tick(id, &head))
    }

    pub fn untick(&mut self, id: &StepId) -> anyhow::Result<()> {
        self.state.untick(id);
        self.unfolded.remove(id);
        self.state_file.update(|saved| saved.untick(id))
    }

    /// A ticked step folds; the reviewer can open it again without losing the tick.
    pub fn is_folded(&self, id: &StepId) -> bool {
        self.is_ticked(id) && !self.unfolded.contains(id)
    }

    pub fn toggle_fold(&mut self, id: &StepId) {
        if !self.unfolded.remove(id) && self.is_ticked(id) {
            self.unfolded.insert(id.clone());
        }
    }

    /// Only steps of this tour count: ticks of steps a refresh removed stay in the file but not here.
    pub fn progress(&self) -> ReadProgress {
        ReadProgress {
            read: self
                .reading_order
                .iter()
                .filter(|id| self.is_ticked(id))
                .count(),
            total: self.reading_order.len(),
        }
    }

    fn position(&self) -> Option<usize> {
        match &self.screen {
            Screen::Summary => None,
            Screen::Step(id) => self.reading_order.iter().position(|step| step == id),
        }
    }
}
