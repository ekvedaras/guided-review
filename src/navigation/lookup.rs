use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use crate::code_source::CodeSource;
use crate::file_path::FilePath;
use crate::language::Language;
use crate::pull_request::Sha;

use super::local::Local;
use super::{
    Answer, AnsweredBy, CodeNavigator, Column, Definition, GitGrep, Import, NavigationRequest,
    Place, Usage, Waiting, Word, WordInFile,
};

/// Finds the files at a commit that hold a word, so only those are parsed.
pub trait FilesWithWord: Send + Sync {
    fn files_with(&self, sha: &Sha, word: &Word) -> anyhow::Result<Vec<FilePath>>;
}

/// Which files a lookup can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    WholeRepo,
    /// A local (a variable or parameter) is looked up in the function that holds it, so no other
    /// file is searched.
    EnclosingFunction,
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
    /// More files hold the word than were parsed, so definitions and usages may be missing.
    pub cut_short: bool,
}

/// How many files a lookup parses at most, so a word as common as `new` cannot stall it.
pub const FILES_PARSED_AT_MOST: usize = 300;

/// What a commit says of a word, the same whichever file asks.
struct Found {
    definitions: Arc<[Definition]>,
    usages: Arc<[Usage]>,
    cut_short: bool,
}

/// Looks words up, each one once per commit: a commit never changes, so what was found stays true.
pub struct Lookups {
    search: FileSearch,
    file_cap: usize,
    found: Mutex<HashMap<(Sha, Word), Arc<Found>>>,
}

impl Lookups {
    pub fn new(search: FileSearch) -> Self {
        Self {
            search,
            file_cap: FILES_PARSED_AT_MOST,
            found: Mutex::new(HashMap::new()),
        }
    }

    pub fn parsing_at_most(mut self, files: usize) -> Self {
        self.file_cap = files;
        self
    }

    /// `code` is read at `sha`; it is lent per call, so a lookup runs with whatever source the
    /// review has now. `from` is the file the word was read in: its own definitions rank first, then those in
    /// files it imports.
    pub fn look_up(
        &self,
        code: &dyn CodeSource,
        sha: &Sha,
        word: &Word,
        from: &FilePath,
    ) -> anyhow::Result<Lookup> {
        let found = self.found_at(code, sha, word)?;
        let imports: Vec<Import> = Self::imports_of(code, sha, from)
            .into_iter()
            .filter(|import| import.name == word.as_str())
            .collect();
        let mut definitions = found.definitions.to_vec();
        definitions.sort_by(|a, b| {
            let rank = |definition: &Definition| {
                if definition.place.path == *from {
                    Rank::SameFile
                } else if imports
                    .iter()
                    .any(|import| import.leads_to(from, &definition.place.path))
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
            cut_short: found.cut_short,
        })
    }

    fn found_at(
        &self,
        code: &dyn CodeSource,
        sha: &Sha,
        word: &Word,
    ) -> anyhow::Result<Arc<Found>> {
        let key = (sha.clone(), word.clone());
        if let Some(found) = self.cache().get(&key) {
            return Ok(Arc::clone(found));
        }
        let found = Arc::new(self.find(code, sha, word)?);
        self.cache().insert(key, Arc::clone(&found));
        Ok(found)
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, HashMap<(Sha, Word), Arc<Found>>> {
        self.found.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn find(&self, code: &dyn CodeSource, sha: &Sha, word: &Word) -> anyhow::Result<Found> {
        let mut definitions = Vec::new();
        let mut usages = Vec::new();
        let (files, cut_short) = self.files_with(code, sha, word)?;
        for (path, source) in files {
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
            cut_short,
        })
    }

    /// The files to parse, and whether more held the word than the cap lets through.
    fn files_with(
        &self,
        code: &dyn CodeSource,
        sha: &Sha,
        word: &Word,
    ) -> anyhow::Result<(Vec<(FilePath, String)>, bool)> {
        let candidates: Vec<FilePath> = match &self.search {
            FileSearch::WholeRepo(search) => search.files_with(sha, word)?,
            FileSearch::ChangedFiles(files) => files.to_vec(),
        };
        let mut parsed: Vec<FilePath> = candidates
            .into_iter()
            .filter(|path| Language::for_path(path.as_str()).is_some())
            .collect();
        let cut_short = parsed.len() > self.file_cap;
        parsed.truncate(self.file_cap);
        // A file that cannot be read holds nothing to find: a changed file is missing on one side
        // of the review (added at head, deleted at base), and one bad read must not lose the rest.
        let files = parsed
            .into_iter()
            .filter_map(|path| {
                let source = code.file_at(sha, &path).ok()?;
                Some((path, source))
            })
            .collect();
        Ok((files, cut_short))
    }

    /// The name that starts at `column` of the line at `at`: a local in its function only, anything
    /// else by name, as `look_up` does.
    pub fn look_up_at(
        &self,
        code: &dyn CodeSource,
        sha: &Sha,
        word: &Word,
        at: &Place,
        column: Column,
    ) -> anyhow::Result<Lookup> {
        let local = Language::for_path(at.path.as_str()).and_then(|language| {
            let source = code.file_at(sha, &at.path).ok()?;
            Local::at(&at.path, &source, language, at.line, column, word)
        });
        let Some(local) = local else {
            return self.look_up(code, sha, word, &at.path);
        };
        Ok(Lookup {
            word: word.clone(),
            definitions: local.definition.into_iter().collect(),
            usages: local.usages.into(),
            reach: Reach::EnclosingFunction,
            cut_short: false,
        })
    }

    /// Without its imports the hovered file still ranks its own definitions first.
    fn imports_of(code: &dyn CodeSource, sha: &Sha, from: &FilePath) -> Vec<Import> {
        let Some(language) = Language::for_path(from.as_str()) else {
            return Vec::new();
        };
        code.file_at(sha, from)
            .map_or_else(|_| Vec::new(), |source| Import::all_in(language, &source))
    }
}

/// The by-name lookup is the first adapter of the code navigator port; it never waits.
impl CodeNavigator for Lookups {
    fn navigate(
        &self,
        code: &dyn CodeSource,
        request: &NavigationRequest,
        _waiting: &dyn Fn(Waiting),
    ) -> anyhow::Result<Answer> {
        let lookup = self.look_up_at(
            code,
            &request.sha,
            &request.word,
            &request.at,
            request.column,
        )?;
        Ok(Answer {
            lookup,
            answered_by: AnsweredBy::ByName { because: None },
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    SameFile,
    /// The hovered file imports the word from the module the definition's file is.
    Imported,
    Elsewhere,
}
