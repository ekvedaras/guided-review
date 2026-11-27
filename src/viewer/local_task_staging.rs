//! What a snapshot scene shows of a review's private tasks: written as the app and `agent-reply` would write them.

use super::comments::{CommentPlace, SendMode};
use super::hand_offs::{AgentReplied, HandOff, SentComment, SentFrom};
use super::local_thread_store::LocalThreadStore;
use super::local_threads::{LocalAuthor, without_last_comment};
use crate::pull_request::Sha;

/// A task the agent answered, written as Send now and `agent-reply` would write it, for a snapshot
/// scene of a local review. Nothing is written when the review has threads already.
pub fn stage_answered_task(
    store: &LocalThreadStore,
    place: &CommentPlace,
    head: &Sha,
    (task, answer): (&str, &str),
) -> anyhow::Result<()> {
    if !store.threads_at(head)?.is_empty() {
        return Ok(());
    }
    let thread = store.start(place, task, head)?;
    store.reply(&thread, answer, LocalAuthor::Agent)?;
    let threads = store.threads_at(head)?;
    let mut answered = Ok(());
    store.file().update(|state| {
        let hand_offs = state.hand_offs_mut();
        hand_offs.record_new_thread(
            place.path(),
            &SentComment {
                place: place.describe(),
                from: SentFrom::File(place.path().clone()),
                mode: SendMode::SendNow,
                body: task,
                code: None,
            },
            Some(&Ok(threads.iter().map(without_last_comment).collect())),
        );
        if let Some(id) = hand_offs.all().last().map(HandOff::id) {
            answered = hand_offs.replied(id, &AgentReplied::default());
        }
    })?;
    Ok(answered?)
}

/// A task added to tasks and not sent yet, written as Add to tasks would write it, for a snapshot
/// scene of a local review. Nothing is written when a thread holds the task already.
pub fn stage_queued_task(
    store: &LocalThreadStore,
    place: &CommentPlace,
    head: &Sha,
    task: &str,
) -> anyhow::Result<()> {
    let staged = store.threads_at(head)?;
    if staged
        .iter()
        .any(|thread| thread.comments.iter().any(|said| said.body == task))
    {
        return Ok(());
    }
    store.start(place, task, head)?;
    let threads = store.threads_at(head)?;
    store.file().update(|state| {
        state.hand_offs_mut().record_new_thread(
            place.path(),
            &SentComment {
                place: place.describe(),
                from: SentFrom::File(place.path().clone()),
                mode: SendMode::AddToTasks,
                body: task,
                code: None,
            },
            Some(&Ok(threads)),
        );
    })
}

/// A task sent to an agent that does not report its state, held back until the reviewer sends it
/// anyway, for a snapshot scene of a local review. Nothing is written when a thread holds the task already.
pub fn stage_held_back_task(
    store: &LocalThreadStore,
    place: &CommentPlace,
    head: &Sha,
    task: &str,
) -> anyhow::Result<()> {
    let staged = store.threads_at(head)?;
    if staged
        .iter()
        .any(|thread| thread.comments.iter().any(|said| said.body == task))
    {
        return Ok(());
    }
    store.start(place, task, head)?;
    let threads = store.threads_at(head)?;
    store.file().update(|state| {
        let hand_offs = state.hand_offs_mut();
        hand_offs.record_new_thread(
            place.path(),
            &SentComment {
                place: place.describe(),
                from: SentFrom::File(place.path().clone()),
                mode: SendMode::SendNow,
                body: task,
                code: None,
            },
            Some(&Ok(threads)),
        );
        hand_offs.hold_back_latest();
    })
}
