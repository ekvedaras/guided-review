//! The language servers of the app: one pool shared by every review, stopped when the app quits.

use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{App, Global};

use crate::app::components::toast_stack::post_toast;
use crate::app_folders::AppFolders;
use crate::codebase::Codebase;
use crate::config::Config;
use crate::github::GitHub;
use crate::navigation::{
    LanguageServers, ManagedServers, MessageKind, ServerCheckouts, ServerFolders, ServerMessage,
    ServerMessages, ServerPins, Timeouts,
};
use crate::update::Target;
use crate::viewer::{LanguageServersRow, Review, ToastTone};

struct ActiveLanguageServers(Arc<LanguageServers>);

impl Global for ActiveLanguageServers {}

/// Hands what servers ask to be shown from their reading threads to the window's toasts.
struct ToastedMessages(UnboundedSender<ServerMessage>);

impl ServerMessages for ToastedMessages {
    fn show(&self, message: ServerMessage) {
        // The receiver ends with the app.
        self.0.unbounded_send(message).ok();
    }
}

fn toast_server_messages(cx: &mut App) -> Arc<dyn ServerMessages> {
    let (messages, mut shown) = unbounded::<ServerMessage>();
    cx.spawn(async move |cx| {
        while let Some(message) = shown.next().await {
            let tone = match message.kind {
                MessageKind::Error => ToastTone::Danger,
                MessageKind::Notice => ToastTone::Neutral,
            };
            let line = format!("{}: {}", message.server, message.text);
            cx.update(|cx| post_toast(cx, tone, line));
        }
    })
    .detach();
    Arc::new(ToastedMessages(messages))
}

/// The answers Settings saved count in every open review at once, and a no stops that codebase's
/// servers.
pub fn answers_saved(rows: &[LanguageServersRow], cx: &App) {
    let Some(ActiveLanguageServers(servers)) = cx.try_global::<ActiveLanguageServers>() else {
        return;
    };
    for row in rows {
        servers.answered(&row.codebase, row.answer);
    }
}

/// Lets the review ask language servers once its reviewer says yes for the repo, which a local review
/// names by its `origin` or else by its folder.
pub fn use_language_servers(review: &mut Review, folders: &AppFolders, cx: &mut App) {
    // A snapshot scene stages its own cards; a question on them would change the pictures.
    if cfg!(feature = "snapshots") {
        return;
    }
    let codebase = Codebase::of(review.subject());
    if !cx.has_global::<ActiveLanguageServers>() {
        let managed = ManagedServers::new(
            ServerFolders {
                downloads: folders.language_servers(),
                indexes: folders.server_indexes(),
            },
            // The servers' releases are on GitHub whatever host the review's PR is on.
            Arc::new(GitHub::from_env()),
            ServerPins::pinned(),
            Target::running(),
        );
        if let Err(error) = managed.remove_unpinned() {
            eprintln!("could not remove old language server downloads: {error:#}");
        }
        let checkouts = ServerCheckouts::new(folders.server_checkouts());
        if let Err(error) = checkouts.remove_local_folder_leftovers(&folders.server_indexes()) {
            eprintln!("could not remove server checkouts of earlier builds: {error:#}");
        }
        let servers = Arc::new(LanguageServers::new(
            Arc::new(managed),
            checkouts,
            Timeouts::default(),
            toast_server_messages(cx),
        ));
        let on_quit = Arc::clone(&servers);
        cx.on_app_quit(move |_| {
            let servers = Arc::clone(&on_quit);
            async move { servers.stop_all() }
        })
        .detach();
        cx.set_global(ActiveLanguageServers(servers));
    }
    let answer = match Config::load_from(folders) {
        Ok(config) => config.language_servers(&codebase),
        Err(error) => {
            eprintln!("could not read config.toml for the language server answer: {error:#}");
            None
        }
    };
    let servers = Arc::clone(&cx.global::<ActiveLanguageServers>().0);
    review.use_language_server_pool(&servers, codebase, answer);
}
