use std::path::{Path, PathBuf};

/// `~/` is the given home, so a setting means the same wherever it is read.
pub fn expand_home(path: &Path, home: &Path) -> PathBuf {
    path.strip_prefix("~")
        .map_or_else(|_| path.to_path_buf(), |rest| home.join(rest))
}
