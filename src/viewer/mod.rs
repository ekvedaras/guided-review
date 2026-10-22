//! The viewer core: everything the app does, with no GPUI in it.
//! Views read its state and send it user actions; tests drive it the same way.

mod highlight;
mod ports;
mod review;
mod review_state;
mod step_page;

pub use highlight::{Capture, HighlightRequest, Highlights, Span};
pub use ports::{AgentPort, GitHubPort};
pub use review::{ReadProgress, Review, Screen, SidebarEntry, StepId};
pub use review_state::{ReadTick, ReviewState, ReviewStateFile};
pub use step_page::{DiffLine, LineLocation, PieceDiff, Side, StepPage};
