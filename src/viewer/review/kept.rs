use std::cell::OnceCell;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use super::ReviewCore;
use crate::viewer::page::Page;

/// The open review, with the page of its screen built once and kept until the review changes.
///
/// The review is read through `Deref` and changed only through `DerefMut`, which drops the kept page
/// first. Its fields are private to this module, so no method can change the review and leave a page
/// built from what it was before: the next `page()` builds it again.
pub struct Review {
    review: ReviewCore,
    page: OnceCell<Arc<Page>>,
}

impl Review {
    pub(super) fn keeping_page_of(review: ReviewCore) -> Self {
        Self {
            review,
            page: OnceCell::new(),
        }
    }

    /// The page of the open screen, as the review is now.
    pub fn page(&self) -> &Arc<Page> {
        self.page.get_or_init(|| Arc::new(Page::of(&self.review)))
    }

    pub(super) fn into_core(self) -> ReviewCore {
        self.review
    }
}

impl Deref for Review {
    type Target = ReviewCore;

    fn deref(&self) -> &ReviewCore {
        &self.review
    }
}

impl DerefMut for Review {
    fn deref_mut(&mut self) -> &mut ReviewCore {
        self.page.take();
        &mut self.review
    }
}
