//! What the page shows of itself as it scrolls: the sticky step bar and file header, the scrollbar,
//! the focused change in view and the diagrams let go off screen.

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    Context, Entity, FontWeight, InteractiveElement, IntoElement, ListOffset, MouseButton,
    ParentElement, Pixels, Styled, Window, div, px,
};

use super::page::{step_place, tick_button};
use super::{OneStepLayout, PAGE_TOP, STICKY_STEP_BAR_HEIGHT};
use crate::app::components::button::ButtonSize;
use crate::app::components::diagram::Diagrams;
use crate::app::components::diff_view::{PIECE_HEADER_HEIGHT, PieceHeader};
use crate::app::components::markdown_view::InlineMarkdown;
use crate::app::components::motion::{BANNER_DROP, Edge, STATE, slide_in};
use crate::app::components::palette::Colors;
use crate::app::components::scrollbar::{ScrollbarDrag, scrollbar};
use crate::viewer::{ChangeInView, ReadStep, Review};

impl OneStepLayout {
    /// Once the step's heading has scrolled away, a slim bar under the title bar keeps where the
    /// reader is and the tick in reach. Read from the list's own scroll state; nothing is built.
    pub(super) fn sticky_step_bar(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let heading = self.session.read(cx).rows().step_heading()?;
        let is_scrolled_away = self.screen_list.logical_scroll_top().item_ix > 0;
        if !is_scrolled_away {
            return None;
        }
        let heading = Arc::clone(heading);
        let colors = Colors::active(cx);
        let bar = div()
            .w_full()
            .h(STICKY_STEP_BAR_HEIGHT)
            .px_6()
            .flex()
            .items_center()
            .gap_3()
            .bg(colors.background.opacity(0.97))
            .border_b_1()
            .border_color(colors.subtle_border)
            .shadow(colors.sticky_shadow())
            .child(step_place(&colors, &heading))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(InlineMarkdown::new("sticky-step-title", &heading.title)),
            )
            .child(tick_button(
                heading.is_ticked,
                heading.id.clone(),
                ButtonSize::Small,
                cx,
            ));
        Some(div().absolute().top_0().left_0().right_0().child(slide_in(
            bar,
            ("sticky-step-bar", self.screen_travel.arrival().count()),
            STATE,
            Edge::Top,
            BANNER_DROP,
        )))
    }

    /// The header of the file box the reader is in stays under the step bar while its code
    /// scrolls under it, naming the symbol there. It takes over when the real header reaches it.
    pub(super) fn pinned_file_header(
        &self,
        under: Pixels,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let pin_line = self.screen_list.viewport_bounds().top() + under + PIECE_HEADER_HEIGHT;
        let pinned = self
            .session
            .read(cx)
            .rows()
            .pinned_header(self.row_at(pin_line))?;
        let mut label = pinned.header.label.clone();
        if pinned.symbol.is_some() {
            label.symbol = pinned.symbol;
        }
        let header = PieceHeader::new(label, pinned.header.repin_label, Vec::new()).pinned();
        let header = match pinned.header.fold.clone() {
            Some(fold) => header.with_fold_toggle(Self::box_fold_toggle(
                fold,
                "pinned-fold-box",
                Some(pinned.header_row),
                cx,
            )),
            None => header,
        };
        let colors = Colors::active(cx);
        Some(
            div()
                .absolute()
                .top(under)
                .left_0()
                .right_0()
                .px_6()
                .bg(colors.background)
                .child(header),
        )
    }

    /// Where the view is on the page, as a thumb on its right edge that drags the page. A press on
    /// the track off the thumb takes the thumb's middle there.
    pub(super) fn page_scrollbar(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let thumb = ScrollbarDrag::thumb(&self.screen_list)?;
        let colors = Colors::active(cx);
        Some(
            scrollbar(
                &colors,
                "page-scrollbar",
                thumb,
                self.page_drag.is_dragging(),
            )
            .top_0()
            .bottom_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.page_drag.press(&this.screen_list, event.position.y);
                    cx.notify();
                }),
            ),
        )
    }

    pub(super) fn is_dragging_a_scrollbar(&self) -> bool {
        self.page_drag.is_dragging() || self.issue_card_drag.is_dragging()
    }

    /// `pointer_y` is in window pixels.
    pub(super) fn drag_scrollbar_to(&mut self, pointer_y: Pixels, cx: &mut Context<Self>) {
        self.page_drag.drag_to(&self.screen_list, pointer_y);
        self.issue_card_drag
            .drag_to(&self.issue_card_scroll, pointer_y);
        cx.notify();
    }

    /// A card dragged by its scrollbar stays while it is dragged; let go off the card, it goes.
    pub(super) fn end_scrollbar_drag(
        &mut self,
        pointer: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.page_drag.release(&self.screen_list);
        if self.issue_card_drag.release(&self.issue_card_scroll)
            && !self.issue_card_scroll.bounds().contains(&pointer)
        {
            self.issue_hover.card = None;
        }
        cx.notify();
    }

    /// The first row of the screen whose bottom is below `y`, in window pixels.
    fn row_at(&self, y: Pixels) -> usize {
        let mut row = self.screen_list.logical_scroll_top().item_ix;
        // gpui reports a row's bounds without the list's top padding, which it paints rows below.
        while self
            .screen_list
            .bounds_for_item(row)
            .is_some_and(|bounds| bounds.bottom() + PAGE_TOP <= y)
        {
            row += 1;
        }
        row
    }

    /// What the sticky step bar and the pinned file header hide at the top of the view once the
    /// page has scrolled.
    fn covered_at_top(&self) -> Pixels {
        if self.screen_list.logical_scroll_top().item_ix > 0 {
            STICKY_STEP_BAR_HEIGHT + PIECE_HEADER_HEIGHT
        } else {
            Pixels::ZERO
        }
    }

    /// The rows the reader sees: from the first that is not under the bars at the top to the last
    /// that starts above the bottom edge. Rows the list has not measured end it.
    fn visible_rows(&self) -> Range<usize> {
        let viewport = self.screen_list.viewport_bounds();
        let first = self.row_at(viewport.top() + self.covered_at_top());
        let mut end = first;
        while self
            .screen_list
            .bounds_for_item(end)
            .is_some_and(|bounds| bounds.top() + PAGE_TOP < viewport.bottom())
        {
            end += 1;
        }
        first..end
    }

    /// Where the rows are in the part of the view the bars leave free. A row the list has not laid
    /// out is far off: above the view when it is before the first row the list shows, else below.
    fn change_in_view(&self, rows: &Range<usize>) -> ChangeInView {
        let viewport = self.screen_list.viewport_bounds();
        let covered = self.covered_at_top();
        let from_free_top =
            |pixels: Pixels| f32::from(pixels + PAGE_TOP - viewport.top() - covered);
        let first_shown = self.screen_list.logical_scroll_top().item_ix;
        let top = match self.screen_list.bounds_for_item(rows.start) {
            Some(bounds) => from_free_top(bounds.top()),
            None if rows.start < first_shown => f32::NEG_INFINITY,
            None => f32::INFINITY,
        };
        let bottom = match rows
            .end
            .checked_sub(1)
            .and_then(|last| self.screen_list.bounds_for_item(last))
        {
            Some(bounds) => from_free_top(bounds.bottom()),
            None => f32::INFINITY,
        };
        ChangeInView {
            view_height: f32::from(viewport.size.height - covered),
            top,
            bottom,
        }
    }

    /// Shows the focused change, scrolling only when it is not all in view already: its top goes
    /// just under the bars.
    fn reveal_focused_change(&mut self, cx: &mut Context<Self>) {
        let Some(rows) = self.session.read(cx).rows().focused_change_rows() else {
            return;
        };
        let view = self.change_in_view(&rows);
        if view.top >= 0.0 && view.bottom <= view.view_height {
            return;
        }
        self.screen_list.scroll_to(ListOffset {
            item_ix: rows.start,
            offset_in_item: Pixels::ZERO,
        });
        self.screen_list
            .scroll_by(-(STICKY_STEP_BAR_HEIGHT + PIECE_HEADER_HEIGHT));
    }

    pub(super) fn focus_change_by(
        &mut self,
        step: impl FnOnce(&mut Review) -> Option<usize>,
        cx: &mut Context<Self>,
    ) {
        self.change_review(cx, step);
        self.reveal_focused_change(cx);
        cx.notify();
    }

    pub(super) fn read_toward(
        &mut self,
        step: impl FnOnce(&mut Review, Option<ChangeInView>) -> ReadStep,
        cx: &mut Context<Self>,
    ) {
        let focused = self
            .session
            .read(cx)
            .rows()
            .focused_change_rows()
            .map(|rows| self.change_in_view(&rows));
        match self.change_review(cx, |review| step(review, focused)) {
            ReadStep::Scroll(pixels) => self.screen_list.scroll_by(px(pixels)),
            ReadStep::Focused(_) => self.reveal_focused_change(cx),
            ReadStep::Nothing => {}
        }
        cx.notify();
    }

    /// The focus goes to a change in view when the one it was on scrolled away.
    pub(super) fn follow_focus_to_scroll(&mut self, cx: &mut Context<Self>) {
        let visible = self.visible_rows();
        if visible.is_empty() || !self.session.read(cx).rows().focus_follows(&visible) {
            return;
        }
        self.session
            .update(cx, |session, cx| session.follow_scroll(visible, cx));
    }

    /// Strips of a diagram scrolled out of the list get no layout, so they are let go here.
    pub(super) fn show_diagram(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let screen_list = &self.screen_list;
        let is_off_view = |row: Option<usize>| {
            row.is_some_and(|row| {
                screen_list.item_is_above_viewport(row) == Some(true)
                    || screen_list.item_is_below_viewport(row) == Some(true)
            })
        };
        let session = self.session.read(cx);
        let rows = session.rows();
        let step_diagram = rows.diagram().map(str::to_owned);
        let step_diagram_is_off_view = is_off_view(rows.diagram_row());
        let description_diagrams: Vec<String> = rows
            .description_diagrams()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let description_is_off_view = is_off_view(rows.description_row());

        Self::show_one(
            &self.diagrams,
            step_diagram.as_deref(),
            step_diagram_is_off_view,
            window,
            cx,
        );
        let sources: Vec<&str> = description_diagrams.iter().map(String::as_str).collect();
        if !self.description_diagrams.is_of(&sources) {
            self.description_diagrams = Self::new_description_diagrams(&sources, cx);
        }
        for (source, diagrams) in self.description_diagrams.clone().iter() {
            Self::show_one(diagrams, Some(source), description_is_off_view, window, cx);
        }
    }

    fn show_one(
        diagrams: &Entity<Diagrams>,
        source: Option<&str>,
        is_off_view: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        diagrams.update(cx, |diagrams, cx| {
            diagrams.show(source, window, cx);
            if is_off_view {
                diagrams.leave_screen(window);
            }
        });
    }
}
