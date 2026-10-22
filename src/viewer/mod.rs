//! The viewer core: everything the app does, with no GPUI in it.
//! Views read its state and send it user actions; tests drive it the same way.

mod agent_event;
mod ask;
mod ask_context;
mod highlight;
mod ports;
mod review;
mod review_state;
mod step_page;

pub use agent_event::AgentEvent;
pub use ask::{Ask, AskId, AskStore, Asks, Author, Message, Place};
pub use highlight::{Capture, HighlightRequest, Highlights, Span};
pub use ports::{AgentLaunch, AgentLauncher, AgentPort, GitHubPort};
pub use review::{ReadProgress, Review, Screen, SidebarEntry, StepId};
pub use review_state::{ReadTick, ReviewState, ReviewStateFile};
pub use step_page::{DiffLine, LineLocation, PieceDiff, Side, StepPage};
