//! Where a name is defined and used at a commit, found by name with tree-sitter alone.

mod git_grep;
mod lookup;
mod word;
mod word_in_file;

pub use git_grep::GitGrep;
pub use lookup::{FILES_PARSED_AT_MOST, FileSearch, FilesWithWord, Lookup, Lookups, Reach};
pub use word::Word;
pub use word_in_file::{Definition, DefinitionKind, Place, Usage, WordInFile};
