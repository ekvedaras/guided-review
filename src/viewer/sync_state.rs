/// Whether a sync with GitHub is under way; a second one asked for meanwhile waits for the first.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    #[default]
    Idle,
    Running,
    RunningThenAgain,
}

impl SyncState {
    /// `true` when the caller starts the sync now; otherwise it will be run once more after the current one.
    pub fn start_or_queue(&mut self) -> bool {
        match self {
            Self::Idle => {
                *self = Self::Running;
                true
            }
            Self::Running | Self::RunningThenAgain => {
                *self = Self::RunningThenAgain;
                false
            }
        }
    }

    /// `true` when another sync was asked for while this one ran.
    pub fn finish_and_wants_again(&mut self) -> bool {
        std::mem::take(self) == Self::RunningThenAgain
    }
}
