//! PROTOTYPE for #137 (a key that shows the changed files as a tree). Throwaway: it lives on the
//! `prototype-137-file-tree` branch only and never goes to main.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::{ReviewCore, StepId};
use crate::coverage;
use crate::file_path::FilePath;
use crate::viewer::file_tree::{FileTree, FolderPath, TreeRowKind};
use crate::viewer::piece_diff::pieces_of;
use crate::viewer::screen::Screen;

#[derive(Debug, Clone)]
pub struct ProtoStep {
    pub id: StepId,
    pub number: usize,
    pub title: String,
    pub is_read: bool,
    pub is_current: bool,
}

#[derive(Debug, Clone)]
pub struct ProtoFile {
    pub path: FilePath,
    pub label: String,
    pub added: usize,
    pub removed: usize,
    pub is_read: bool,
    pub steps: Vec<ProtoStep>,
    pub in_safety_net: bool,
    pub on_screen: bool,
}

#[derive(Debug, Clone)]
pub enum ProtoRowKind {
    Folder {
        path: FolderPath,
        label: String,
        is_folded: bool,
        added: usize,
        removed: usize,
        is_read: bool,
    },
    File(ProtoFile),
}

#[derive(Debug, Clone)]
pub struct ProtoRow {
    pub depth: usize,
    pub kind: ProtoRowKind,
}

#[derive(Debug, Clone, Default)]
pub struct ProtoFileTree {
    pub rows: Vec<ProtoRow>,
    pub file_count: usize,
    pub added: usize,
    pub removed: usize,
    pub is_guided: bool,
}

impl ReviewCore {
    pub fn prototype_file_tree(&self, folded: &BTreeSet<FolderPath>) -> ProtoFileTree {
        let guided = self.tour.guided();
        let current_step = match &self.screen {
            Screen::Step(id) => Some(id),
            _ => None,
        };
        let mut steps_of: HashMap<&FilePath, Vec<ProtoStep>> = HashMap::new();
        let mut safety_net: HashSet<&FilePath> = HashSet::new();
        if let Some(guided) = guided {
            for (index, step) in guided.steps().into_iter().enumerate() {
                let proto = ProtoStep {
                    id: step.id.clone(),
                    number: index + 1,
                    title: step.title.clone(),
                    is_read: self.is_ticked(&step.id),
                    is_current: current_step == Some(&step.id),
                };
                let mut seen = HashSet::new();
                for piece in pieces_of(step, guided, &self.files) {
                    let path = piece.id.path();
                    if seen.insert(path) {
                        steps_of.entry(path).or_default().push(proto.clone());
                    }
                }
            }
            if self.reading_order().contains(&Screen::SafetyNet) {
                safety_net = coverage::safety_net(guided, &self.files)
                    .iter()
                    .map(|part| part.piece.id.path())
                    .collect();
            }
        }
        let file_of = |path: &FilePath, label: String| {
            let changed = self.files.iter().find(|file| &file.path == path);
            let steps = steps_of.get(path).cloned().unwrap_or_default();
            let in_safety_net = safety_net.contains(path);
            ProtoFile {
                path: path.clone(),
                label,
                added: changed.map_or(0, |file| file.added),
                removed: changed.map_or(0, |file| file.removed),
                is_read: if guided.is_some() {
                    !steps.is_empty() && steps.iter().all(|step| step.is_read)
                } else {
                    self.is_file_ticked(path)
                },
                on_screen: match &self.screen {
                    Screen::File(shown) => shown == path,
                    Screen::Step(_) => steps.iter().any(|step| step.is_current),
                    Screen::SafetyNet => in_safety_net,
                    Screen::Summary | Screen::Overview => false,
                },
                in_safety_net,
                steps,
            }
        };
        let tree = FileTree::of(self.files.iter().map(|file| &file.path));
        let all_files: Vec<ProtoFile> = tree
            .reading_order()
            .iter()
            .map(|path| file_of(path, path.to_string()))
            .collect();
        let under = |folder: &FolderPath| {
            let prefix = format!("{folder}/");
            all_files
                .iter()
                .filter(move |file| file.path.as_str().starts_with(&prefix))
        };
        let rows = tree
            .rows(folded, None, &|_| false, &|_| None)
            .into_iter()
            .map(|row| ProtoRow {
                depth: row.depth,
                kind: match row.kind {
                    TreeRowKind::Folder {
                        path,
                        label,
                        is_folded,
                        ..
                    } => ProtoRowKind::Folder {
                        added: under(&path).map(|file| file.added).sum(),
                        removed: under(&path).map(|file| file.removed).sum(),
                        is_read: under(&path).all(|file| file.is_read),
                        path,
                        label,
                        is_folded,
                    },
                    TreeRowKind::File { path, label, .. } => {
                        ProtoRowKind::File(file_of(&path, label))
                    }
                },
            })
            .collect();
        ProtoFileTree {
            rows,
            file_count: all_files.len(),
            added: all_files.iter().map(|file| file.added).sum(),
            removed: all_files.iter().map(|file| file.removed).sum(),
            is_guided: guided.is_some(),
        }
    }
}
