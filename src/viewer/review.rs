use std::fmt;

use crate::code_source::CodeSource;
use crate::pieces::{self, ChangedFile};
use crate::pull_request::Sha;
use crate::tour::{Step, Tour};

use super::step_page::StepPage;

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

pub struct Review {
    tour: Tour,
    files: Vec<ChangedFile>,
    /// Parent, its children, then the next parent: from the big picture to the details.
    reading_order: Vec<StepId>,
    screen: Screen,
}

impl Review {
    /// Code is read at the tour's own base and head, not the PR's current ones.
    pub fn open(tour: Tour, source: &dyn CodeSource) -> anyhow::Result<Self> {
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
        })
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
        Some(StepPage::of(step, &self.tour, &self.files))
    }

    pub fn sidebar(&self) -> Vec<SidebarEntry<'_>> {
        fn walk<'a>(
            steps: &'a [Step],
            depth: usize,
            current: Option<&StepId>,
            into: &mut Vec<SidebarEntry<'a>>,
        ) {
            for step in steps {
                into.push(SidebarEntry {
                    id: StepId::from(step.id.as_str()),
                    title: &step.title,
                    depth,
                    is_current: current.is_some_and(|id| id.0 == step.id),
                });
                walk(&step.children, depth + 1, current, into);
            }
        }
        let current = match &self.screen {
            Screen::Summary => None,
            Screen::Step(id) => Some(id),
        };
        let mut entries = Vec::new();
        walk(&self.tour.steps, 0, current, &mut entries);
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

    fn position(&self) -> Option<usize> {
        match &self.screen {
            Screen::Summary => None,
            Screen::Step(id) => self.reading_order.iter().position(|step| step == id),
        }
    }
}
