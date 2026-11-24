use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::Context as _;

use crate::code_source::CodeSource;
use crate::file_path::FilePath;
use crate::language::Language;
use crate::pull_request::Sha;

use super::{Definition, GitGrep, Usage, Word, WordInFile};

/// Finds the files at a commit that hold a word, so only those are parsed.
pub trait FilesWithWord: Send + Sync {
    fn files_with(&self, sha: &Sha, word: &Word) -> anyhow::Result<Vec<FilePath>>;
}

/// Which files a lookup can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    WholeRepo,
    /// The code is read from GitHub, so only the review's changed files are searched.
    ChangedFiles,
}

/// Which files a lookup parses.
pub enum FileSearch {
    WholeRepo(Arc<dyn FilesWithWord>),
    ChangedFiles(Arc<[FilePath]>),
}

impl FileSearch {
    pub fn whole_repo(search: Arc<dyn FilesWithWord>) -> Self {
        Self::WholeRepo(search)
    }

    pub fn changed_files(files: Arc<[FilePath]>) -> Self {
        Self::ChangedFiles(files)
    }

    /// The whole repo with a local clone, else only the review's changed files.
    pub fn for_source(source: &dyn CodeSource, changed_files: Arc<[FilePath]>) -> Self {
        match source.local_clone() {
            Some(clone) => Self::in_clone(clone),
            None => Self::changed_files(changed_files),
        }
    }

    fn in_clone(clone: &Path) -> Self {
        Self::whole_repo(Arc::new(GitGrep::in_clone(clone)))
    }

    fn reach(&self) -> Reach {
        match self {
            Self::WholeRepo(_) => Reach::WholeRepo,
            Self::ChangedFiles(_) => Reach::ChangedFiles,
        }
    }
}

/// Where a word is defined and used at a commit, definitions best match first.
#[derive(Debug, Clone)]
pub struct Lookup {
    pub word: Word,
    pub definitions: Vec<Definition>,
    /// By file, then line.
    pub usages: Arc<[Usage]>,
    pub reach: Reach,
}

/// What a commit says of a word, the same whichever file asks.
struct Found {
    definitions: Arc<[Definition]>,
    usages: Arc<[Usage]>,
}

/// Looks words up, each one once per commit: a commit never changes, so what was found stays true.
pub struct Lookups {
    code: Arc<dyn CodeSource>,
    search: FileSearch,
    found: Mutex<HashMap<(Sha, Word), Arc<Found>>>,
}

impl Lookups {
    pub fn new(code: Arc<dyn CodeSource>, search: FileSearch) -> Self {
        Self {
            code,
            search,
            found: Mutex::new(HashMap::new()),
        }
    }

    /// `from` is the file the word was read in: its own definitions rank first, then those in
    /// files it imports.
    pub fn look_up(&self, sha: &Sha, word: &Word, from: &FilePath) -> anyhow::Result<Lookup> {
        let found = self.found_at(sha, word)?;
        let imported = self.imported_names(sha, from)?;
        let mut definitions = found.definitions.to_vec();
        definitions.sort_by(|a, b| {
            let rank = |definition: &Definition| {
                if definition.place.path == *from {
                    Rank::SameFile
                } else if imported
                    .iter()
                    .any(|name| name == file_stem(&definition.place.path))
                {
                    Rank::Imported
                } else {
                    Rank::Elsewhere
                }
            };
            (rank(a), &a.place.path, a.place.line).cmp(&(rank(b), &b.place.path, b.place.line))
        });
        Ok(Lookup {
            word: word.clone(),
            definitions,
            usages: Arc::clone(&found.usages),
            reach: self.search.reach(),
        })
    }

    fn found_at(&self, sha: &Sha, word: &Word) -> anyhow::Result<Arc<Found>> {
        let key = (sha.clone(), word.clone());
        if let Some(found) = self.cache().get(&key) {
            return Ok(Arc::clone(found));
        }
        let found = Arc::new(self.find(sha, word)?);
        self.cache().insert(key, Arc::clone(&found));
        Ok(found)
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, HashMap<(Sha, Word), Arc<Found>>> {
        self.found.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn find(&self, sha: &Sha, word: &Word) -> anyhow::Result<Found> {
        let mut definitions = Vec::new();
        let mut usages = Vec::new();
        for (path, source) in self.files_with(sha, word)? {
            let Some(language) = Language::for_path(path.as_str()) else {
                continue;
            };
            let (defined, used) = WordInFile::read(&path, &source, language, word).into_parts();
            definitions.extend(defined);
            usages.extend(used);
        }
        usages.sort_by(|a, b| (&a.place.path, a.place.line).cmp(&(&b.place.path, b.place.line)));
        Ok(Found {
            definitions: definitions.into(),
            usages: usages.into(),
        })
    }

    fn files_with(&self, sha: &Sha, word: &Word) -> anyhow::Result<Vec<(FilePath, String)>> {
        match &self.search {
            FileSearch::WholeRepo(search) => search
                .files_with(sha, word)?
                .into_iter()
                .filter(|path| Language::for_path(path.as_str()).is_some())
                .map(|path| {
                    let source = self
                        .code
                        .file_at(sha, &path)
                        .with_context(|| format!("reading {path} at {sha} for {word}"))?;
                    Ok((path, source))
                })
                .collect(),
            // A changed file is missing on one side of the review: added at head, deleted at
            // base. A file that cannot be read is one of those and holds nothing to find.
            FileSearch::ChangedFiles(files) => Ok(files
                .iter()
                .filter(|path| Language::for_path(path.as_str()).is_some())
                .filter_map(|path| Some((path.clone(), self.code.file_at(sha, path).ok()?)))
                .collect()),
        }
    }

    fn imported_names(&self, sha: &Sha, from: &FilePath) -> anyhow::Result<Vec<String>> {
        let Some(language) = Language::for_path(from.as_str()) else {
            return Ok(Vec::new());
        };
        let source = self
            .code
            .file_at(sha, from)
            .with_context(|| format!("reading {from} at {sha} for its imports"))?;
        Ok(language.imported_names(&source))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    SameFile,
    /// In a file the hovered file imports: its name is one the imports give.
    Imported,
    Elsewhere,
}

/// `src/cart.ts` gives `cart`.
fn file_stem(path: &FilePath) -> &str {
    Path::new(path.as_str())
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
}
