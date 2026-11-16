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
use toml_edit::visit_mut::VisitMut;
use toml_edit::{DocumentMut, Formatted, Value};

use crate::platform;
use crate::update::ReleaseInstall;

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
            None => Self::xdg(),
        }
    }

    /// Run once at start: moves `~/.guided-review` when it can, and makes the result this process's folders.
    pub fn settle() -> anyhow::Result<Self> {
        let xdg = Self::xdg()?;
        let settled = Self::move_once(xdg.clone(), Mover::of_this_process(&xdg.home));
        Ok(RUNNING.get_or_init(|| settled).clone())
    }

    /// Moves `~/.guided-review` again, for a start that found an app of an earlier build in it and has
    /// since asked that app to quit.
    pub fn settle_again(&self) -> anyhow::Result<Self> {
        if *self == Self::before_xdg(&self.home) {
            Ok(Self::move_once(
                Self::xdg()?,
                Mover::of_this_process(&self.home),
            ))
        } else {
            Ok(self.clone())
        }
    }

    /// The XDG folders of this process's environment, moved into or not.
    pub fn xdg() -> anyhow::Result<Self> {
        let home = std::env::home_dir().context("could not find the home folder")?;
        Ok(platform::app_folders(&home, |name| std::env::var_os(name)))
    }

    /// The folders a start uses: `xdg`, after each file `~/.guided-review` holds that `xdg` lacks is
    /// moved into it, or the old folder while that cannot be done: an app of an earlier version holds
    /// its lock, or a step failed and what moved was put back. A file both hold stays in each, and
    /// `files_not_read` names it. A move never fails the start: what could not be put back stays where
    /// it is, `files_not_read` names that too, and the next start tries the move again.
    pub fn move_once(xdg: Self, mover: Mover) -> Self {
        let old = Self::before_xdg(&xdg.home);
        let moves: Vec<(PathBuf, PathBuf)> = MOVED
            .iter()
            .map(|place| (place(&old), place(&xdg)))
            .collect();
        if !moves.iter().any(|(from, to)| left_to_move(from, to)) {
            return xdg;
        }
        let not_moved = |xdg: Self| if xdg.holds_state() { xdg } else { old.clone() };
        if mover == Mover::AnotherBinary {
            return not_moved(xdg);
        }
        let _lock = match take_lock(&old.lock()) {
            Ok(OldLock::Taken(lock)) => lock,
            Ok(OldLock::HeldByAnotherApp) => return not_moved(xdg),
            Ok(OldLock::FolderGone) => return xdg,
            Err(error) => {
                eprintln!("warning: {error:#}; nothing was moved");
                return not_moved(xdg);
            }
        };
        let mut moved = Vec::new();
        match moves
            .iter()
            .try_for_each(|(from, to)| merge(from, to, &mut moved))
        {
            Ok(()) => {
                if let Err(error) = old.point_config_at_moved_files(&xdg) {
                    eprintln!("warning: {error:#}; it may name files in their old place");
                }
                old.tidy_after_move();
                xdg
            }
            Err(error) => {
                eprintln!(
                    "warning: could not move {} into the XDG folders, so it stays in use: {error:#}",
                    old.state.display()
                );
                for (from, to) in moved.iter().rev() {
                    if let Err(error) = merge(to, from, &mut Vec::new()) {
                        eprintln!(
                            "warning: could not put {} back after a failed move, so it stays in {}; the next start tries the move again: {error:#}",
                            from.display(),
                            to.display()
                        );
                    }
                }
                xdg.remove_if_empty();
                old
            }
        }
    }

    /// What the app keeps that lies in the folders it does not use, each beside the place the app
    /// uses for it instead: what a move could not put back, or what an older binary wrote into
    /// `~/.guided-review` after the move. `self` is the folders in use, `xdg` those of the move.
    pub fn files_not_read(&self, xdg: &Self) -> Vec<NotRead> {
        let old = Self::before_xdg(&self.home);
        let unused = if *self == old { xdg } else { &old };
        MOVED
            .iter()
            .flat_map(|place| {
                let (unused_place, used_place) = (place(unused), place(self));
                files_under(&unused_place)
                    .into_iter()
                    .filter_map(move |file| {
                        Some(NotRead {
                            read_instead: rebased(&file, &unused_place, &used_place)?,
                            file,
                        })
                    })
            })
            .collect()
    }

    /// Config values naming a file the move took, such as
    /// `summary_instructions = "~/.guided-review/summary.md"`, name its new place. Quotes, comments
    /// and the rest stay as the reviewer wrote them.
    fn point_config_at_moved_files(&self, xdg: &Self) -> anyhow::Result<()> {
        let path = xdg.config_file();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(());
        };
        let mut document: DocumentMut = text
            .parse()
            .with_context(|| format!("could not read {}", path.display()))?;
        let mut repointing = Repointing {
            old: self,
            xdg,
            changed: false,
        };
        repointing.visit_document_mut(&mut document);
        if !repointing.changed {
            return Ok(());
        }
        let written = partial(&path);
        std::fs::write(&written, document.to_string())
            .with_context(|| format!("could not write {}", written.display()))?;
        std::fs::rename(&written, &path)
            .with_context(|| format!("could not move {} to {}", written.display(), path.display()))
    }

    /// Where `written`, a path in a config value, is after the move, written the same way: under `~/`
    /// when it was. `None` when it names no file the move took.
    fn moved_place(&self, xdg: &Self, written: &str) -> Option<String> {
        let from_home = written.strip_prefix("~/");
        let path = from_home.map_or_else(|| PathBuf::from(written), |rest| self.home.join(rest));
        let moved = MOVED
            .iter()
            .find_map(|place| rebased(&path, &place(self), &place(xdg)))?;
        Some(match moved.strip_prefix(&self.home) {
            Ok(rest) if from_home.is_some() => format!("~/{}", rest.display()),
            _ => moved.display().to_string(),
        })
    }

    /// Whether a move put what the app wrote here: a config alone may be written by hand.
    fn holds_state(&self) -> bool {
        [Self::tours, Self::state_files, Self::evals, Self::log]
            .iter()
            .any(|place| !files_under(&place(self)).is_empty())
    }

    /// Best effort: what is left is a lock and socket of no running app and a cache that fills again,
    /// and a folder that cannot be removed only stays as it is.
    fn tidy_after_move(&self) {
        for leftover in [
            self.state.join(MOVING_MARK_OF_EARLIER_BUILDS),
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

/// The outcome of deleting `path`: a file or folder that was never there is already gone.
pub fn remove_if_there(path: &Path, removed: std::io::Result<()>) -> anyhow::Result<()> {
    match removed {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("could not delete {}", path.display())),
    }
}

/// Which binary moves `~/.guided-review`. A maintainer runs source builds beside the release install,
/// and a move by one of them would leave the other on a folder no longer used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mover {
    /// The release install, or any binary on a machine with none.
    ThisBinary,
    /// Uses the XDG folders once the move happened, the old folder before.
    AnotherBinary,
}

impl Mover {
    fn of_this_process(home: &Path) -> Self {
        let install = ReleaseInstall::in_home(home);
        match platform::running_binary() {
            Ok(exe) => install.folder_mover(&exe),
            Err(_) => install.folder_mover(Path::new("")),
        }
    }
}

/// A file the app does not read, as it uses `read_instead`, which may not exist. Nothing merges the
/// two: the reviewer keeps what they need by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotRead {
    pub file: PathBuf,
    pub read_instead: PathBuf,
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

/// Left in the old folder by a move of a build before each file was moved on its own; a move now
/// finishes whatever is left, so it is only tidied away.
const MOVING_MARK_OF_EARLIER_BUILDS: &str = ".moving";

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

/// Whether `from` holds a file a move would still take: one `to` lacks, or holds the same of.
fn left_to_move(from: &Path, to: &Path) -> bool {
    if !exists(from) {
        return false;
    }
    if !exists(to) {
        return true;
    }
    if is_dir(from) && is_dir(to) {
        return names_in(from).is_ok_and(|names| {
            names
                .iter()
                .any(|name| left_to_move(&from.join(name), &to.join(name)))
        });
    }
    same(from, to).unwrap_or(false)
}

/// Moves what `from` holds into `to` one file or folder `to` lacks at a time, and notes each in
/// `moved`, so a failed move can put exactly those back. What `to` already holds is never written
/// over: a file in both stays in both, unless the two are the same.
fn merge(from: &Path, to: &Path, moved: &mut Vec<(PathBuf, PathBuf)>) -> anyhow::Result<()> {
    if is_dir(from) && is_dir(to) {
        for name in names_in(from)? {
            merge(&from.join(&name), &to.join(&name), moved)?;
        }
        // Stays when a file in both is left in it.
        std::fs::remove_dir(from).ok();
        return Ok(());
    }
    if !exists(from) || (exists(to) && !same(from, to)?) {
        return Ok(());
    }
    relocate(from, to)?;
    moved.push((from.to_path_buf(), to.to_path_buf()));
    Ok(())
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
        let left_names = names_in(left)?;
        if left_names != names_in(right)? {
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

/// `path` under `to` as it is under `from`; `None` when it is not under `from`.
fn rebased(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(from).ok()?;
    Some(if rest.as_os_str().is_empty() {
        to.to_path_buf()
    } else {
        to.join(rest)
    })
}

/// Walks a config, `old` being the folders before the move.
struct Repointing<'a> {
    old: &'a AppFolders,
    xdg: &'a AppFolders,
    changed: bool,
}

impl VisitMut for Repointing<'_> {
    fn visit_string_mut(&mut self, node: &mut Formatted<String>) {
        let Some(moved) = self.old.moved_place(self.xdg, node.value()) else {
            return;
        };
        let literal = node.display_repr().starts_with('\'') && !moved.contains(['\'', '\n']);
        let mut fresh = if literal {
            match format!("'{moved}'").parse::<Value>() {
                Ok(Value::String(fresh)) => fresh,
                _ => Formatted::new(moved),
            }
        } else {
            Formatted::new(moved)
        };
        *fresh.decor_mut() = node.decor().clone();
        *node = fresh;
        self.changed = true;
    }
}

fn names_in(folder: &Path) -> anyhow::Result<BTreeSet<OsString>> {
    std::fs::read_dir(folder)
        .and_then(|entries| entries.map(|entry| entry.map(|e| e.file_name())).collect())
        .with_context(|| format!("could not read {}", folder.display()))
}

/// The files at `path` and in the folders under it; none when nothing is there.
fn files_under(path: &Path) -> Vec<PathBuf> {
    if !is_dir(path) {
        return if exists(path) {
            vec![path.to_path_buf()]
        } else {
            Vec::new()
        };
    }
    names_in(path)
        .unwrap_or_default()
        .iter()
        .flat_map(|name| files_under(&path.join(name)))
        .collect()
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn is_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
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
