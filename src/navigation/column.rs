/// Where a name starts in its line, in bytes from the line's start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Column(u32);

impl Column {
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<usize> for Column {
    type Error = std::num::TryFromIntError;

    fn try_from(byte: usize) -> Result<Self, Self::Error> {
        u32::try_from(byte).map(Self)
    }
}
