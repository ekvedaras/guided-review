use std::collections::HashSet;

use crate::pieces::{ChangedFile, Piece};
use crate::tour::{PieceRef, Tour};

impl PieceRef {
    /// The pieces this pointer names directly; `None` when it names something that did not change.
    /// `rest` names nothing directly, it only mops up what is left.
    pub fn named_pieces<'a>(&self, files: &'a [ChangedFile]) -> Option<Vec<&'a Piece>> {
        match self {
            Self::Rest { .. } => Some(Vec::new()),
            Self::Lines { file, .. } | Self::WholeFile { file } => files
                .iter()
                .find(|changed| changed.path == *file)
                .map(|changed| changed.pieces.iter().collect()),
            Self::Symbol { file, symbol } => files
                .iter()
                .flat_map(|changed| &changed.pieces)
                .find(|piece| {
                    piece.id.path == *file && piece.id.symbol.as_deref() == Some(symbol.as_str())
                })
                .map(|piece| vec![piece]),
        }
    }
}

/// Changes no step covers. A `rest` pointer covers everything under its folder
/// that is not already covered, so it only matters for what no other pointer names.
pub fn uncovered<'a>(tour: &Tour, files: &'a [ChangedFile]) -> Vec<&'a Piece> {
    let steps = tour.steps();
    let pointers: Vec<&PieceRef> = steps.iter().flat_map(|step| &step.pieces).collect();
    let named: HashSet<String> = pointers
        .iter()
        .filter_map(|pointer| pointer.named_pieces(files))
        .flatten()
        .map(|piece| piece.id.to_string())
        .collect();
    let rest_folders: Vec<&str> = pointers
        .iter()
        .filter_map(|pointer| match pointer {
            PieceRef::Rest { rest } => Some(rest.trim_matches('/')),
            _ => None,
        })
        .collect();

    files
        .iter()
        .filter(|file| {
            !rest_folders
                .iter()
                .any(|folder| is_under(&file.path, folder))
        })
        .flat_map(|file| &file.pieces)
        .filter(|piece| !named.contains(&piece.id.to_string()))
        .collect()
}

fn is_under(path: &str, folder: &str) -> bool {
    folder.is_empty()
        || path
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
}
