use std::collections::HashMap;
use std::fmt;

use anyhow::bail;

use crate::code_source::CodeSource;
use crate::pieces::{self, Piece, PieceId, PieceKind};
use crate::pull_request::Sha;
use crate::tour::{PieceRef, Step, Tour};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepinStatus {
    Same,
    Changed,
    Gone,
    New,
}

impl fmt::Display for RepinStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Same => "same",
            Self::Changed => "changed",
            Self::Gone => "gone",
            Self::New => "new",
        })
    }
}

/// What happened to one piece between the previous head and the new one.
/// `id` is the new id, except for gone pieces, which only have an old one.
#[derive(Debug, Clone)]
pub struct PieceRepin {
    pub id: PieceId,
    pub status: RepinStatus,
}

#[derive(Debug, Default)]
pub struct Repin {
    pub pieces: Vec<PieceRepin>,
}

impl Repin {
    pub fn count(&self, status: RepinStatus) -> usize {
        self.pieces
            .iter()
            .filter(|piece| piece.status == status)
            .count()
    }
}

impl fmt::Display for Repin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for piece in &self.pieces {
            writeln!(f, "{} {}", piece.status, piece.id)?;
        }
        writeln!(
            f,
            "{} same, {} changed, {} gone, {} new",
            self.count(RepinStatus::Same),
            self.count(RepinStatus::Changed),
            self.count(RepinStatus::Gone),
            self.count(RepinStatus::New),
        )
    }
}

/// Moves the tour to the new commits and says what became of each piece.
/// The old pieces are cut again at the tour's own base and head, because the tour file keeps no hashes.
pub fn repin(
    tour: &mut Tour,
    source: &dyn CodeSource,
    new_base: &Sha,
    new_head: &Sha,
) -> anyhow::Result<Repin> {
    if tour.base.is_empty() || tour.head.is_empty() {
        bail!("the tour is not pinned to commits yet; run `guided-review check` first");
    }
    let old_files = pieces::cut(source, &Sha::new(&tour.base), &Sha::new(&tour.head))?;
    let new_files = pieces::cut(source, new_base, new_head)?;
    let old: Vec<&Piece> = old_files.iter().flat_map(|file| &file.pieces).collect();
    let new: Vec<&Piece> = new_files.iter().flat_map(|file| &file.pieces).collect();

    let matching = Matching::between(&old, &new);

    if tour.head != new_head.as_str() {
        tour.previous_head = Some(std::mem::replace(&mut tour.head, new_head.to_string()));
    }
    tour.base = new_base.to_string();
    retarget(
        &mut tour.steps,
        &matching.new_id_of_old_hunk,
        &matching.gone_hunks,
    );

    let mut pieces: Vec<PieceRepin> = new
        .iter()
        .map(|piece| PieceRepin {
            id: piece.id.clone(),
            status: matching
                .status_of_new
                .get(&piece.id.to_string())
                .copied()
                .unwrap_or(RepinStatus::New),
        })
        .collect();
    pieces.extend(
        old.iter()
            .filter(|piece| matching.gone_ids.contains(&piece.id.to_string()))
            .map(|piece| PieceRepin {
                id: piece.id.clone(),
                status: RepinStatus::Gone,
            }),
    );
    Ok(Repin { pieces })
}

struct Matching {
    status_of_new: HashMap<String, RepinStatus>,
    gone_ids: Vec<String>,
    new_id_of_old_hunk: HashMap<String, String>,
    gone_hunks: Vec<String>,
}

impl Matching {
    fn between(old: &[&Piece], new: &[&Piece]) -> Self {
        let mut matching = Self {
            status_of_new: HashMap::new(),
            gone_ids: Vec::new(),
            new_id_of_old_hunk: HashMap::new(),
            gone_hunks: Vec::new(),
        };
        let is_hunk = |piece: &&Piece| piece.kind == PieceKind::Hunk;

        let new_by_id: HashMap<String, &Piece> = new
            .iter()
            .filter(|piece| !is_hunk(piece))
            .map(|piece| (piece.id.to_string(), *piece))
            .collect();
        for piece in old.iter().filter(|piece| !is_hunk(piece)) {
            let id = piece.id.to_string();
            match new_by_id.get(&id) {
                Some(now) => {
                    let status = if now.diff_hash == piece.diff_hash {
                        RepinStatus::Same
                    } else {
                        RepinStatus::Changed
                    };
                    matching.status_of_new.insert(id, status);
                }
                None => matching.gone_ids.push(id),
            }
        }

        // Hunk ids are ordinals that shift when a hunk is added above, so hunks are matched
        // by their diff hash first, and only then by position among what is left in the file.
        let mut unmatched_old: Vec<&Piece> = old.iter().copied().filter(is_hunk).collect();
        let mut unmatched_new: Vec<&Piece> = new.iter().copied().filter(is_hunk).collect();
        for pair in [HunkPairing::SameHash, HunkPairing::SamePositionInFile] {
            unmatched_new.retain(|now| {
                let found = unmatched_old
                    .iter()
                    .position(|before| pair.matches(before, now));
                let Some(index) = found else { return true };
                let before = unmatched_old.remove(index);
                matching
                    .new_id_of_old_hunk
                    .insert(before.id.to_string(), now.id.to_string());
                matching.status_of_new.insert(
                    now.id.to_string(),
                    if before.diff_hash == now.diff_hash {
                        RepinStatus::Same
                    } else {
                        RepinStatus::Changed
                    },
                );
                false
            });
        }
        for before in unmatched_old {
            matching.gone_ids.push(before.id.to_string());
            matching.gone_hunks.push(before.id.to_string());
        }
        matching
    }
}

#[derive(Clone, Copy)]
enum HunkPairing {
    SameHash,
    SamePositionInFile,
}

impl HunkPairing {
    fn matches(self, before: &Piece, now: &Piece) -> bool {
        before.id.path == now.id.path
            && match self {
                Self::SameHash => before.diff_hash == now.diff_hash,
                Self::SamePositionInFile => true,
            }
    }
}

/// Points hunk pointers at the ordinal their hunk has now; pointers to vanished hunks are dropped,
/// because that ordinal may now belong to another hunk.
fn retarget(steps: &mut [Step], new_id_of_old_hunk: &HashMap<String, String>, gone: &[String]) {
    for step in steps {
        step.pieces.retain_mut(|pointer| {
            let PieceRef::Symbol { file, symbol } = pointer else {
                return true;
            };
            let old_id = format!("{file}#{symbol}");
            if gone.contains(&old_id) {
                return false;
            }
            if let Some(new_id) = new_id_of_old_hunk.get(&old_id) {
                *symbol = new_id[file.len() + 1..].to_string();
            }
            true
        });
        retarget(&mut step.children, new_id_of_old_hunk, gone);
    }
}
