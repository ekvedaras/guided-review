//! The language servers of the app: one pool shared by every review, stopped when the app quits.

use std::sync::Arc;

use gpui::{App, Global};

use crate::app_folders::AppFolders;
use crate::config::Config;
use crate::github::GitHub;
use crate::navigation::{LanguageServers, ManagedServers, ServerPins, Timeouts};
use crate::update::Target;
use crate::viewer::Review;

struct ActiveLanguageServers(Arc<LanguageServers>);

impl Global for ActiveLanguageServers {}

/// Lets the review ask language servers once its reviewer says yes for the repo. A review with no
/// GitHub repo has none to ask about.
pub fn use_language_servers(review: &mut Review, folders: &AppFolders, cx: &mut App) {
    // A snapshot scene stages its own cards; a question on them would change the pictures.
    if cfg!(feature = "snapshots") {
        return;
    }
    let Some(repo) = review.subject().pull_request().map(|pr| pr.repo.clone()) else {
        return;
    };
    if !cx.has_global::<ActiveLanguageServers>() {
        let managed = ManagedServers::new(
            folders.language_servers(),
            Arc::new(GitHub::from_env()),
            ServerPins::pinned(),
            Target::running(),
        );
        if let Err(error) = managed.remove_unpinned() {
            eprintln!("could not remove old language server downloads: {error:#}");
        }
        let servers = Arc::new(LanguageServers::new(Arc::new(managed), Timeouts::default()));
        let on_quit = Arc::clone(&servers);
        cx.on_app_quit(move |_| {
            let servers = Arc::clone(&on_quit);
            async move { servers.stop_all() }
        })
        .detach();
        cx.set_global(ActiveLanguageServers(servers));
    }
    let answer = match Config::load_from(folders) {
        Ok(config) => config.language_servers(&repo),
        Err(error) => {
            eprintln!("could not read config.toml for the language server answer: {error:#}");
            None
        }
    };
    let servers = Arc::clone(&cx.global::<ActiveLanguageServers>().0);
    review.use_language_server_pool(&servers, repo, answer);
}
