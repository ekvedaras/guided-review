//! The viewer core: everything the app does, with no GPUI in it.
//! Views read its state and send it user actions; tests drive it the same way.

mod ports;
mod review;
mod step_page;

pub use ports::{AgentPort, GitHubPort};
pub use review::{Review, Screen, SidebarEntry, StepId};
pub use step_page::{DiffLine, LineLocation, PieceDiff, Side, StepPage};
