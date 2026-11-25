//! Where a shown line lives: its file, the side of the diff and its number. Comments, code links,
//! asks and names all start from one.

use serde::{Deserialize, Serialize};

use crate::file_path::FilePath;
use crate::findings::AnchorSide;
use crate::line_number::LineNumber;
use crate::pieces::ChangedFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Base,
    Head,
}

impl From<AnchorSide> for Side {
    fn from(side: AnchorSide) -> Self {
        match side {
            AnchorSide::Base => Self::Base,
            AnchorSide::Head => Self::Head,
        }
    }
}

/// Where a shown line lives. Comments and code links start from this.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LineLocation {
    /// GitHub addresses both sides of a renamed file by its new path.
    pub path: FilePath,
    pub side: Side,
    pub line: LineNumber,
}

impl ChangedFile {
    /// Where the file is on `side`: at base, a renamed file's old path.
    pub fn path_on(&self, side: Side) -> &FilePath {
        match side {
            Side::Base => self.old_path.as_ref().unwrap_or(&self.path),
            Side::Head => &self.path,
        }
    }
}
