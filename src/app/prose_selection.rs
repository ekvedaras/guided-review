//! The prose the reader drags over in a markdown body (a comment, a step's text), kept for the whole
//! app so any body can draw its part and the copy action can read it.

use std::collections::HashMap;
use std::ops::Range;

use gpui::{App, Global, SharedString};

use super::code_selection::{ProseSelection, TextSpot};

#[derive(Default)]
pub struct ProseSelections {
    selection: Option<ProseSelection>,
    /// Each drawn body's texts in reading order, as last drawn, so a copy has their words.
    texts: HashMap<SharedString, Vec<SharedString>>,
}

impl Global for ProseSelections {}

impl ProseSelections {
    pub fn install(cx: &mut App) {
        cx.set_global(Self::default());
    }

    fn of(cx: &mut App) -> &mut Self {
        cx.default_global::<Self>()
    }

    /// A body starts drawing: its texts are counted from the first again.
    pub fn body_drawn(body: &SharedString, cx: &mut App) {
        Self::of(cx).texts.insert(body.clone(), Vec::new());
    }

    /// Notes the next text of `body` and returns its row, its place in the body's reading order.
    pub fn text_drawn(body: &SharedString, text: SharedString, cx: &mut App) -> usize {
        let texts = Self::of(cx).texts.entry(body.clone()).or_default();
        texts.push(text);
        texts.len() - 1
    }

    pub fn range_on(
        body: &SharedString,
        row: usize,
        length: usize,
        cx: &App,
    ) -> Option<Range<usize>> {
        cx.try_global::<Self>()?
            .selection
            .as_ref()
            .filter(|selection| selection.is_on(body))?
            .range_on(row, length)
    }

    pub fn press(body: SharedString, at: TextSpot, cx: &mut App) {
        Self::of(cx).selection = Some(ProseSelection::start(body, at));
    }

    pub fn press_twice(body: &SharedString, at: TextSpot, cx: &mut App) {
        let selections = Self::of(cx);
        let word = selections
            .texts
            .get(body)
            .and_then(|texts| texts.get(at.row))
            .and_then(|text| ProseSelection::word(body.clone(), at, text));
        selections.selection = word;
    }

    pub fn drag_over(body: &SharedString, at: TextSpot, cx: &mut App) {
        if let Some(selection) = &mut Self::of(cx).selection
            && selection.is_on(body)
        {
            selection.drag_to(at);
        }
    }

    /// The mouse was let go: a press that never moved selects nothing.
    pub fn end_drag(cx: &mut App) {
        let selections = Self::of(cx);
        if let Some(selection) = &mut selections.selection
            && selection.is_dragging()
            && !selection.end_drag()
        {
            selections.selection = None;
        }
    }

    /// A press anywhere drops a selection that is done; one being dragged is kept.
    pub fn clear_unless_dragging(cx: &mut App) -> bool {
        let selections = Self::of(cx);
        let is_done = selections
            .selection
            .as_ref()
            .is_some_and(|selection| !selection.is_dragging());
        if is_done {
            selections.selection = None;
        }
        is_done
    }

    /// The selected text, a line per text of the body; `None` when nothing is selected.
    pub fn text(cx: &App) -> Option<String> {
        let selections = cx.try_global::<Self>()?;
        let selection = selections.selection.as_ref()?;
        let texts = selections
            .texts
            .iter()
            .find(|(body, _)| selection.is_on(body))?
            .1;
        let text = selection.text(|row| texts.get(row).map(SharedString::as_ref));
        (!text.is_empty()).then_some(text)
    }
}
