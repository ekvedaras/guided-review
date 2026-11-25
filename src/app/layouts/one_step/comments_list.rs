//! The comments list: going to the thread a row names.

use gpui::{Context, ListOffset, px};

use super::OneStepLayout;
use crate::app::code_links::open_link;
use crate::app::components::comments_sheet::CommentsEvent;
use crate::viewer::{ThreadId, ThreadVisit};

impl OneStepLayout {
    /// The list closed itself: the page shows the thread's screen, scrolled to it, or its comment opens on GitHub.
    pub(super) fn comments_list_asked(&mut self, event: &CommentsEvent, cx: &mut Context<Self>) {
        let CommentsEvent::Open(id) = event;
        let visit = self.session.update(cx, |session, cx| {
            session.change(cx, |review| review.go_to_thread(id))
        });
        match visit {
            ThreadVisit::OnScreen => self.scroll_to_thread(id, cx),
            ThreadVisit::OnHost(url) => open_link(&url, cx),
            ThreadVisit::Nowhere => {}
        }
    }

    /// An open list follows the threads as GitHub's answers change them.
    pub(super) fn refresh_comments_list(&mut self, cx: &mut Context<Self>) {
        if !self.comments_list.read(cx).is_open() {
            return;
        }
        let list = self.session.read(cx).review().comments_list();
        self.comments_list
            .update(cx, |sheet, cx| sheet.refresh(&list, cx));
    }

    fn scroll_to_thread(&mut self, id: &ThreadId, cx: &mut Context<Self>) {
        let row = self.session.read(cx).rows().row_of_thread(id);
        self.screen_list.scroll_to(ListOffset {
            item_ix: row.unwrap_or(0),
            offset_in_item: px(0.),
        });
        cx.notify();
    }
}
