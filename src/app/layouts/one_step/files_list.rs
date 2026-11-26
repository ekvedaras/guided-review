//! The files list: going to the file a row names, on the screen picked for it.

use gpui::Context;

use super::OneStepLayout;
use crate::app::components::files_sheet::FilesEvent;

impl OneStepLayout {
    /// The list closed itself: the page shows the screen, scrolled to the file's box.
    pub(super) fn files_list_asked(&mut self, event: &FilesEvent, cx: &mut Context<Self>) {
        let FilesEvent::Go(target) = event;
        self.jump_to(target, cx);
        cx.notify();
    }

    /// An open list follows the read ticks and the screen on show.
    pub(super) fn refresh_files_list(&mut self, cx: &mut Context<Self>) {
        if !self.files_list.read(cx).is_open() {
            return;
        }
        let list = self.session.read(cx).review().files_list();
        self.files_list
            .update(cx, |sheet, cx| sheet.refresh(&list, cx));
    }
}
