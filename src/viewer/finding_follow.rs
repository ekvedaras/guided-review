//! Findings after new commits: they move with their lines to the new head, as local threads do, and one on
//! lines a commit changed is outdated rather than dropped, since a new commit often fixes one finding and
//! leaves the rest true.

use crate::code_source::CodeSource;
use crate::findings::{Anchor, AnchorSide, Finding, FindingsFile, Outdated};
use crate::line_range::LineRange;
use crate::pull_request::Sha;
use crate::review_subject::ReviewSubject;

use super::outdated_lines::LineFollower;

/// Moves every finding about lines from the commits the file is pinned to onto `base` and `head`, and pins
/// the file there. A file the review agent has not had pinned yet is left alone. Reads the code, so it runs
/// off the main thread.
pub(super) fn follow_findings(
    file: &mut FindingsFile,
    subject: &ReviewSubject,
    source: &(impl CodeSource + ?Sized),
    base: &Sha,
    head: &Sha,
) {
    if !file.pin.is_pinned() {
        return;
    }
    let (old_base, old_head) = (file.pin.base.clone(), file.pin.head.clone());
    let mut follower = LineFollower::new(source);
    for finding in &mut file.findings {
        let (from, to) = match &finding.anchor {
            Anchor::Lines {
                side: AnchorSide::Head,
                ..
            } => (&old_head, head),
            Anchor::Lines {
                side: AnchorSide::Base,
                ..
            } => (&old_base, base),
            Anchor::Change | Anchor::File { .. } => continue,
        };
        if from != to {
            follow(finding, &mut follower, source, from, to);
        }
    }
    file.pin.pin_to(subject, base, head);
}

fn follow<S: CodeSource + ?Sized>(
    finding: &mut Finding,
    follower: &mut LineFollower<'_, S>,
    source: &S,
    from: &Sha,
    to: &Sha,
) {
    let Anchor::Lines { file, lines, .. } = &mut finding.anchor else {
        return;
    };
    match follower.follow(file, *lines, from, to) {
        Some(followed) => {
            *lines = LineRange::new(followed.start, followed.end);
            if followed.were_changed && finding.outdated.is_none() {
                finding.outdated = Some(Outdated {
                    before: followed.before,
                });
            }
        }
        None if finding.outdated.is_none() => {
            let before = source.file_at(from, file).map_or_else(
                |_| Vec::new(),
                |text| {
                    text.lines()
                        .skip(usize::from(lines.start).saturating_sub(1))
                        .take(usize::from(lines.end).saturating_sub(usize::from(lines.start)) + 1)
                        .map(str::to_owned)
                        .collect()
                },
            );
            finding.outdated = Some(Outdated { before });
        }
        None => {}
    }
}
