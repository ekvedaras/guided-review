//! A server's answers read as a lookup of the repo: its locations as places at the reviewed head, or
//! as files on disk, and its hover as the first definition's signature and doc.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::code_source::CodeSource;
use crate::file_path::FilePath;
use crate::line_number::LineNumber;
use crate::pull_request::Sha;

use super::hover::{Hovered, kind_of};
use super::positions::path_of;
use super::{Definition, Lookup, Place, Reach, Usage, Word};

/// Turns the server's locations into places of the repo.
pub(super) struct Places<'a> {
    pub(super) code: &'a dyn CodeSource,
    /// The server checkout the server's locations are under.
    pub(super) checkout: &'a Path,
    pub(super) sha: &'a Sha,
}

struct Found {
    place: Place,
    /// The last line of the location's range: a declaration's range can start at its doc comment.
    through: LineNumber,
    /// Set when the file is not in the repo's git, so a click opens it from disk.
    on_disk: Option<PathBuf>,
}

impl Places<'_> {
    pub(super) fn lookup(
        &self,
        word: &Word,
        definitions: &Value,
        usages: &Value,
        hover: &Value,
    ) -> Lookup {
        let hovered = Hovered::of(hover);
        let mut on_disk: Vec<(FilePath, PathBuf)> = Vec::new();
        let mut keep = |found: &Found| {
            if let Some(path) = &found.on_disk
                && !on_disk.iter().any(|(file, _)| *file == found.place.path)
            {
                on_disk.push((found.place.path.clone(), path.clone()));
            }
        };
        let mut sources: HashMap<FilePath, Option<String>> = HashMap::new();
        let mut definition_list = Vec::new();
        for (index, found) in locations_of(definitions)
            .filter_map(|at| self.place_of(at))
            .take(10)
            .enumerate()
        {
            let found = self.at_name(&mut sources, found, word);
            keep(&found);
            let line_text = self.line_at(&mut sources, &found).unwrap_or_default();
            let (signature, doc) = match (index, &hovered.signature) {
                (0, Some(signature)) => (signature.clone(), hovered.doc.clone()),
                _ => (line_text, None),
            };
            definition_list.push(Definition {
                name: word.clone(),
                kind: kind_of(&signature),
                place: found.place,
                signature: signature.into(),
                signature_origin: Arc::new([]),
                doc: doc.map(Into::into),
            });
        }
        let mut usage_list = Vec::new();
        for found in locations_of(usages).filter_map(|at| self.place_of(at)) {
            keep(&found);
            if let Some(text) = self.line_at(&mut sources, &found) {
                usage_list.push(Usage {
                    place: found.place,
                    text: text.into(),
                });
            }
        }
        usage_list
            .sort_by(|a, b| (&a.place.path, a.place.line).cmp(&(&b.place.path, b.place.line)));
        Lookup {
            word: word.clone(),
            definitions: definition_list,
            usages: usage_list.into(),
            reach: Reach::WholeRepo,
            cut_short: false,
            on_disk_files: on_disk.into(),
        }
    }

    fn place_of(&self, location: &Value) -> Option<Found> {
        let uri = location["uri"]
            .as_str()
            .or_else(|| location["targetUri"].as_str())?;
        let range = if location["range"].is_null() {
            &location["targetSelectionRange"]
        } else {
            &location["range"]
        };
        let line_at = |end: &str| {
            LineNumber::try_from(
                u32::try_from(range.pointer(&format!("/{end}/line"))?.as_u64()?)
                    .ok()?
                    .checked_add(1)?,
            )
            .ok()
        };
        let line = line_at("start")?;
        let through = line_at("end").unwrap_or(line);
        let absolute = path_of(uri)?;
        let inside = absolute
            .strip_prefix(self.checkout)
            .ok()
            .map(Path::to_path_buf)
            .or_else(|| {
                let root = self.checkout.canonicalize().ok()?;
                absolute
                    .canonicalize()
                    .ok()?
                    .strip_prefix(root)
                    .ok()
                    .map(Path::to_path_buf)
            });
        let tracked = inside
            .as_deref()
            .and_then(|relative| FilePath::new(&relative.to_string_lossy()).ok())
            .filter(|path| self.code.blob_info_at(self.sha, path).is_ok());
        let (path, on_disk) = if let Some(path) = tracked {
            (path, None)
        } else {
            let label = inside.unwrap_or_else(|| absolute.clone());
            let label = label.to_string_lossy();
            let path = FilePath::new(label.trim_start_matches('/')).ok()?;
            (path, Some(absolute))
        };
        Some(Found {
            place: Place { path, line },
            through,
            on_disk,
        })
    }

    /// Moves a definition to the first line of its range that names it outside a comment.
    fn at_name(
        &self,
        sources: &mut HashMap<FilePath, Option<String>>,
        mut found: Found,
        word: &Word,
    ) -> Found {
        let first = usize::from(found.place.line);
        let last = usize::from(found.through);
        let named = (first..=last).find_map(|number| {
            let line = LineNumber::try_from(number).ok()?;
            let at = Found {
                place: Place {
                    path: found.place.path.clone(),
                    line,
                },
                through: line,
                on_disk: found.on_disk.clone(),
            };
            let text = self.line_at(sources, &at)?;
            let comment = ["*", "/", "#"].iter().any(|start| text.starts_with(start));
            (!comment && text.contains(word.as_str())).then_some(line)
        });
        if let Some(line) = named {
            found.place.line = line;
        }
        found
    }

    fn line_at(
        &self,
        sources: &mut HashMap<FilePath, Option<String>>,
        found: &Found,
    ) -> Option<String> {
        let source =
            sources
                .entry(found.place.path.clone())
                .or_insert_with(|| match &found.on_disk {
                    Some(disk) => std::fs::read_to_string(disk).ok(),
                    None => self.code.file_at(self.sha, &found.place.path).ok(),
                });
        let index = usize::from(found.place.line).checked_sub(1)?;
        Some(source.as_deref()?.lines().nth(index)?.trim().to_owned())
    }
}

/// A server answers with one location, a list of them, or nothing.
fn locations_of(answer: &Value) -> impl Iterator<Item = &Value> {
    let list: &[Value] = match answer {
        Value::Array(list) => list,
        Value::Object(_) => std::slice::from_ref(answer),
        _ => &[],
    };
    list.iter()
}
