#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a panic in a test helper is the test failing"
)]
#![allow(
    clippy::needless_pass_by_value,
    reason = "test helpers take owned arguments so a case reads as a plain call"
)]

mod support;

mod active_pr;
mod app_link;
mod ask;
mod ask_adapters;
mod ask_lifecycle;
mod check;
mod code_links;
mod code_source;
mod comment_actions;
mod diagrams;
mod diff_hunks;
mod eval;
mod eval_adapters;
mod file_path;
mod file_reads;
mod finish_review;
mod github_threads;
mod highlighting;
mod ide;
mod image_previews;
mod jump;
mod line_number;
mod markdown;
mod new_commits;
mod open;
mod open_app;
mod open_review;
mod outline;
mod outline_extras;
mod outline_images;
mod outline_symbols;
mod pick_code_source;
mod piece_id;
mod pr_description;
mod read_ticks;
mod refresh;
mod repin;
mod review_threads;
mod show;
mod step_details;
mod symbol_edges;
mod timestamp;
mod tour_agent;
mod tour_file;
mod viewed_sync;
mod viewer;
mod write_comments;
