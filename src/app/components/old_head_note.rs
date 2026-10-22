use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

use crate::pull_request::Sha;
use crate::viewer::OldHead;

use super::palette::Palette;

/// Says the tour may be stale because the PR has new commits.
#[derive(IntoElement)]
pub struct OldHeadNote {
    old_head: OldHead,
}

impl OldHeadNote {
    pub fn new(old_head: OldHead) -> Self {
        Self { old_head }
    }
}

impl RenderOnce for OldHeadNote {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .px_6()
            .py_2()
            .bg(Palette::current())
            .border_b_1()
            .border_color(Palette::border())
            .text_sm()
            .child(format!(
                "This tour is for commit {}, but the PR is now at {}. The tour may be out of date.",
                short(&self.old_head.tour_head),
                short(&self.old_head.pr_head)
            ))
    }
}

fn short(sha: &Sha) -> &str {
    sha.as_str().get(..7).unwrap_or(sha.as_str())
}
