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
mod agent_reply;
mod app_link;
mod app_state;
mod app_window;
mod ask;
mod ask_adapters;
mod ask_lifecycle;
mod check;
mod code_links;
mod code_source;
mod comment_actions;
mod contrast;
mod diagrams;
mod diff_hunks;
mod eval;
mod eval_adapters;
mod fake_app;
mod file_path;
mod file_reads;
mod finish_review;
mod github_releases;
mod github_threads;
mod hand_offs;
mod hc_tour_agent;
mod highlighting;
mod icons;
mod ide;
mod image_previews;
mod jump;
mod line_number;
mod markdown;
mod new_commits;
mod open;
mod open_app;
mod open_review;
mod outdated_threads;
mod outline;
mod outline_extras;
mod outline_images;
mod outline_symbols;
mod pick_code_source;
mod piece_id;
mod pr_description;
mod pr_notes;
mod pr_switcher;
mod problems;
mod read_ticks;
mod refresh;
mod release_install;
mod relevant_prs;
mod reopen_after_save;
mod repin;
mod repin_guard;
mod restart_guard;
mod review_threads;
mod screen_travel;
mod settings;
mod shortcuts;
mod show;
mod step_details;
mod summary_instructions;
mod symbol_edges;
mod timestamp;
mod toasts;
mod tour_agent;
mod tour_file;
mod tour_skill;
mod tour_writing;
mod updates;
mod viewed_sync;
mod viewer;
mod write_comments;
