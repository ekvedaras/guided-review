//! Where the app keeps what it writes for one reviewer: config, state and cache folders.
//! The platform gives their layout; before XDG they were all `~/.guided-review`.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};

use crate::platform;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppFolders {
    home: PathBuf,
    config: PathBuf,
    state: PathBuf,
    cache: PathBuf,
}

impl AppFolders {
    pub fn new(home: &Path, config: PathBuf, state: PathBuf, cache: PathBuf) -> Self {
        Self {
            home: home.to_path_buf(),
            config,
            state,
            cache,
        }
    }

    /// This process's folders, as `settle` left them. Before it runs (as in tests) the XDG folders as
    /// they are, so nothing reads them for a move.
    pub fn running() -> anyhow::Result<Self> {
        match RUNNING.get() {
            Some(folders) => Ok(folders.clone()),
            None => Self::xdg_of_this_process(),
        }
    }

    /// Run once at start: moves `~/.guided-review` when it can, and makes the result this process's folders.
    pub fn settle() -> anyhow::Result<Self> {
        let settled = Self::move_once(Self::xdg_of_this_process()?)?;
        Ok(RUNNING.get_or_init(|| settled).clone())
    }

    /// Moves `~/.guided-review` again, for a start that found an app of an earlier build in it and has
    /// since asked that app to quit.
    pub fn settle_again(&self) -> anyhow::Result<Self> {
        if *self == Self::before_xdg(&self.home) {
            Self::move_once(platform::app_folders(&self.home, |name| {
                std::env::var_os(name)
            }))
        } else {
            Ok(self.clone())
        }
    }

    fn xdg_of_this_process() -> anyhow::Result<Self> {
        let home = std::env::home_dir().context("could not find the home folder")?;
        Ok(platform::app_folders(&home, |name| std::env::var_os(name)))
    }

    /// The folders a start uses: `xdg`, after what `~/.guided-review` holds is moved into it, or the
    /// old folder while that cannot be done: an app of an earlier version holds its lock, or a step
    /// failed and everything was put back. Never some of each. Once the XDG folders hold files, the old
    /// folder is not read again, even if an older binary writes it.
    pub fn move_once(xdg: Self) -> anyhow::Result<Self> {
        let old = Self::before_xdg(&xdg.home);
        let mark = old.state.join(MOVING_MARK);
        let moves: Vec<(PathBuf, PathBuf)> = MOVED
            .iter()
            .map(|place| (place(&old), place(&xdg)))
            .collect();
        if !exists(&mark) && !moves.iter().any(|(from, _)| exists(from)) {
            return Ok(xdg);
        }
        let moved_before = |xdg: &Self| !exists(&mark) && xdg.holds_files();
        let _lock = match take_lock(&old.lock())? {
            OldLock::Taken(lock) => lock,
            OldLock::HeldByAnotherApp => {
                return Ok(if moved_before(&xdg) { xdg } else { old });
            }
            OldLock::FolderGone => return Ok(xdg),
        };
        if moved_before(&xdg) {
            // Another start moved it while this one waited, and opening the lock made the folder again.
            old.tidy_after_move();
            return Ok(xdg);
        }
        if let Err(error) = std::fs::write(&mark, "") {
            eprintln!(
                "warning: could not start moving {} into the XDG folders, so it stays in use: {error}",
                old.state.display()
            );
            return Ok(old);
        }
        match moves.iter().try_for_each(|(from, to)| relocate(from, to)) {
            Ok(()) => {
                old.tidy_after_move();
                Ok(xdg)
            }
            Err(error) => {
                eprintln!(
                    "warning: could not move {} into the XDG folders, so it stays in use: {error:#}",
                    old.state.display()
                );
                for (from, to) in moves.iter().rev() {
                    relocate(to, from).with_context(|| {
                        format!(
                            "could not put {} back after a failed move; the next start finishes the move",
                            from.display()
                        )
                    })?;
                }
                discard(&mark)?;
                xdg.remove_if_empty();
                Ok(old)
            }
        }
    }

    /// `~/.guided-review` when, after the move, something wrote what the app keeps back into it: an
    /// older binary that still uses it. Its files are not read, so the reviewer must know.
    pub fn old_folder_written_again(&self) -> Option<PathBuf> {
        let old = Self::before_xdg(&self.home);
        if *self == old {
            return None;
        }
        MOVED
            .iter()
            .map(|place| place(&old))
            .chain([old.lock()])
            .any(|path| exists(&path))
            .then_some(old.state)
    }

    fn holds_files(&self) -> bool {
        [&self.config, &self.state].into_iter().any(|folder| {
            std::fs::read_dir(folder).is_ok_and(|mut entries| entries.next().is_some())
        })
    }

    /// Best effort: what is left is a lock and socket of no running app and a cache that fills again,
    /// and a folder that cannot be removed only stays as it is.
    fn tidy_after_move(&self) {
        for leftover in [
            self.state.join(MOVING_MARK),
            self.lock(),
            self.socket(),
            self.cache.clone(),
        ] {
            if let Err(error) = discard(&leftover) {
                eprintln!("warning: {error:#}");
            }
        }
        std::fs::remove_dir(&self.state).ok();
    }

    /// After a failed move was put back: the folders it made, if nothing else is in them.
    fn remove_if_empty(&self) {
        for folder in [&self.config, &self.state] {
            std::fs::remove_dir(folder).ok();
        }
    }

    pub fn before_xdg(home: &Path) -> Self {
        let old = home.join(".guided-review");
        Self::new(home, old.clone(), old.clone(), old.join("cache"))
    }

    /// For `~/` in settings, and for what lives under home whatever the folders are.
    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    pub fn summary_file(&self) -> PathBuf {
        self.config.join("summary.md")
    }

    /// Where tours live unless `GUIDED_REVIEW_TOURS_DIR` says otherwise (see `Tour::tours_dir`).
    pub fn tours(&self) -> PathBuf {
        self.state.join("tours")
    }

    /// The state file of each PR, under owner folders, beside the app's own.
    pub fn state_files(&self) -> PathBuf {
        self.state.join("state")
    }

    pub fn evals(&self) -> PathBuf {
        self.state.join("evals")
    }

    pub fn lock(&self) -> PathBuf {
        self.state.join("app.lock")
    }

    pub fn log(&self) -> PathBuf {
        self.state.join("app.log")
    }

    pub fn images(&self) -> PathBuf {
        self.cache.join("images")
    }

    /// Beside the lock, not in the runtime folder: a CLI started with another environment (an agent,
    /// ssh) must find the socket of the running app.
    pub fn socket(&self) -> PathBuf {
        self.state.join("app.sock")
    }
}

static RUNNING: OnceLock<AppFolders> = OnceLock::new();

/// What `~/.guided-review` held that the app still needs, each at its place in the folders. The cache
/// is left out, as it fills again; the lock and socket belong to whichever app runs.
const MOVED: [fn(&AppFolders) -> PathBuf; 6] = [
    AppFolders::config_file,
    AppFolders::summary_file,
    AppFolders::tours,
    AppFolders::state_files,
    AppFolders::evals,
    AppFolders::log,
];

/// In the old folder while a move runs, so a start after a crash finishes it rather than taking the
/// half-filled XDG folders as moved.
const MOVING_MARK: &str = ".moving";

/// A move takes milliseconds; an app of an earlier version holds the lock for as long as it runs.
const LOCK_WAIT: Duration = Duration::from_secs(1);
const LOCK_POLL: Duration = Duration::from_millis(25);

enum OldLock {
    Taken(File),
    HeldByAnotherApp,
    /// Another start moved everything and removed the folder.
    FolderGone,
}

/// The app lock of the old folder, so no app of an earlier version writes it during the move.
fn take_lock(path: &Path) -> anyhow::Result<OldLock> {
    let lock = match OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
    {
        Ok(lock) => lock,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(OldLock::FolderGone),
        Err(error) => {
            return Err(error).with_context(|| format!("could not open {}", path.display()));
        }
    };
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(OldLock::Taken(lock)),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(LOCK_POLL);
            }
            Err(TryLockError::WouldBlock) => return Ok(OldLock::HeldByAnotherApp),
            Err(TryLockError::Error(error)) => {
                return Err(error).with_context(|| format!("could not lock {}", path.display()));
            }
        }
    }
}

/// Moves a file or folder with a rename, so it is never in two halves. Across disks it copies, checks
/// the copy, puts it in place under its name, and only then deletes the original. Done again after a
/// crash, it finishes what was left.
fn relocate(from: &Path, to: &Path) -> anyhow::Result<()> {
    discard(&partial(from))?;
    discard(&partial(to))?;
    if !exists(from) {
        return Ok(());
    }
    if exists(to) {
        // A copy across disks was put in place and the original not yet deleted.
        if !same(from, to)? {
            bail!(
                "{} and {} both exist and differ",
                from.display(),
                to.display()
            );
        }
        return discard(from);
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::CrossesDevices => copy_checked(from, to),
        Err(error) => Err(error)
            .with_context(|| format!("could not move {} to {}", from.display(), to.display())),
    }
}

fn copy_checked(from: &Path, to: &Path) -> anyhow::Result<()> {
    let copy = partial(to);
    copy_tree(from, &copy)
        .with_context(|| format!("could not copy {} to {}", from.display(), copy.display()))?;
    if !same(from, &copy)? {
        discard(&copy)?;
        bail!("the copy of {} differs from it", from.display());
    }
    std::fs::rename(&copy, to)
        .with_context(|| format!("could not move {} to {}", copy.display(), to.display()))?;
    discard(from)
}

fn partial(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".partial");
    path.with_file_name(name)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    let kind = std::fs::symlink_metadata(from)?.file_type();
    if kind.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let name = entry?.file_name();
            copy_tree(&from.join(&name), &to.join(&name))?;
        }
        Ok(())
    } else if kind.is_symlink() {
        std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
    } else {
        std::fs::copy(from, to).map(drop)
    }
}

fn same(left: &Path, right: &Path) -> anyhow::Result<bool> {
    let read = |path: &Path| {
        std::fs::symlink_metadata(path)
            .map(|meta| meta.file_type())
            .with_context(|| format!("could not read {}", path.display()))
    };
    let (left_kind, right_kind) = (read(left)?, read(right)?);
    if left_kind.is_dir() && right_kind.is_dir() {
        let names = |path: &Path| -> anyhow::Result<BTreeSet<OsString>> {
            std::fs::read_dir(path)
                .and_then(|entries| entries.map(|entry| entry.map(|e| e.file_name())).collect())
                .with_context(|| format!("could not read {}", path.display()))
        };
        let left_names = names(left)?;
        if left_names != names(right)? {
            return Ok(false);
        }
        for name in left_names {
            if !same(&left.join(&name), &right.join(&name))? {
                return Ok(false);
            }
        }
        Ok(true)
    } else if left_kind.is_symlink() && right_kind.is_symlink() {
        Ok(std::fs::read_link(left)? == std::fs::read_link(right)?)
    } else if left_kind.is_file() && right_kind.is_file() {
        let bytes = |path: &Path| {
            std::fs::read(path).with_context(|| format!("could not read {}", path.display()))
        };
        Ok(bytes(left)? == bytes(right)?)
    } else {
        Ok(false)
    }
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn discard(path: &Path) -> anyhow::Result<()> {
    let removed = match std::fs::symlink_metadata(path) {
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(());
        }
        Err(error) => Err(error),
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    };
    removed.with_context(|| format!("could not remove {}", path.display()))
}
