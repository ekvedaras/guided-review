use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{App, AppContext, Context, Entity, EventEmitter, Global, SharedString, Task};

use crate::github::GitHub;
use crate::update::{
    InTheWay, Installed, ReleaseInstall, SkillLink, Target, Update, UpdateOffer, Version,
    find_update,
};
use crate::viewer::ToastTone;

use super::components::toast_stack::post_toast;

/// Long enough that opening a PR has GitHub to itself first.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_hours(6);

/// The app's updates. They are there only when this binary is a release install: one built from
/// source has no link to move, and a release older than it would only be in the way.
#[derive(Clone)]
pub struct Updates(pub Entity<Updater>);

impl Global for Updates {}

impl Updates {
    pub fn install(home: &Path, cx: &mut App) {
        let install = ReleaseInstall::in_home(home);
        let is_release_install = std::env::current_exe().is_ok_and(|exe| install.runs(&exe));
        if !is_release_install {
            return;
        }
        let updater = cx.new(|cx| Updater {
            install,
            offer: UpdateOffer::default(),
            action: None,
            _checks: Updater::check_now_and_then(cx),
            installing: None,
        });
        cx.set_global(Self(updater));
    }

    /// An update offered without asking GitHub, for the snapshot run.
    #[cfg(feature = "snapshots")]
    pub fn stage(home: &Path, offer: UpdateOffer, cx: &mut App) {
        let updater = cx.new(|cx| {
            let mut updater = Updater {
                install: ReleaseInstall::in_home(home),
                offer,
                action: None,
                _checks: Task::ready(()),
                installing: None,
            };
            updater.offer_changed(cx);
            updater
        });
        cx.set_global(Self(updater));
    }
}

/// Looked up again for each review opened, so removing the binary named clears it.
pub fn release_install_in_the_way() -> Option<InTheWay> {
    let home = std::env::home_dir()?;
    let exe = std::env::current_exe().ok()?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    ReleaseInstall::in_home(&home).in_the_way(&exe, &path)
}

/// An update is installed and the app should restart into it.
pub struct UpdateInstalled {
    /// The new release's own binary.
    pub program: PathBuf,
}

pub struct Updater {
    install: ReleaseInstall,
    offer: UpdateOffer,
    /// What the update button says, made when the offer changes rather than every frame.
    action: Option<SharedString>,
    _checks: Task<()>,
    installing: Option<Task<()>>,
}

impl EventEmitter<UpdateInstalled> for Updater {}

impl Updater {
    fn check_now_and_then(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIRST_CHECK_AFTER).await;
            loop {
                let found = cx
                    .background_spawn(async {
                        find_update(&GitHub::from_env(), Version::running(), Target::running())
                    })
                    .await;
                let updated = this.update(cx, |updater, cx| {
                    match found {
                        Ok(update) => updater.offer.found(update),
                        Err(error) => eprintln!("could not check for an update: {error:#}"),
                    }
                    updater.offer_changed(cx);
                });
                if updated.is_err() {
                    return;
                }
                cx.background_executor().timer(CHECK_EVERY).await;
            }
        })
    }

    /// What the update button says, when there is an update to show.
    pub fn action(&self) -> Option<&SharedString> {
        self.action.as_ref()
    }

    pub fn is_installing(&self) -> bool {
        matches!(self.offer, UpdateOffer::Installing(_))
    }

    /// Installs the offered update. Once it is installed, asks for the restart again, since the first
    /// may have waited for an Ask or hand-off in flight.
    pub fn act(&mut self, cx: &mut Context<Self>) {
        if let UpdateOffer::Installed(installed) = &self.offer {
            cx.emit(UpdateInstalled {
                program: installed.program.clone(),
            });
            return;
        }
        let Some(update) = self.offer.start_install() else {
            return;
        };
        self.offer_changed(cx);
        let install = self.install.clone();
        let version = update.version;
        let installed = cx.background_spawn(async move {
            install.download_and_install(&GitHub::from_env(), &update)
        });
        self.installing = Some(cx.spawn(async move |this, cx| {
            let result = installed.await;
            this.update(cx, |updater, cx| updater.install_ended(version, result, cx))
                .ok();
        }));
    }

    fn install_ended(
        &mut self,
        version: Version,
        result: anyhow::Result<Installed>,
        cx: &mut Context<Self>,
    ) {
        self.installing = None;
        let ended = self.offer.install_ended(result);
        self.offer_changed(cx);
        if let Err(error) = ended {
            post_toast(
                cx,
                ToastTone::Danger,
                format!("Could not update to {version}: {error:#}"),
            );
            return;
        }
        let UpdateOffer::Installed(installed) = &self.offer else {
            return;
        };
        if installed.skill == SkillLink::NotOurs {
            post_toast(
                cx,
                ToastTone::Neutral,
                format!(
                    "Updated to {version}. The tour skill in ~/.claude/skills/tour is not a link to a \
                     release, so it was left as it is."
                ),
            );
        }
        cx.emit(UpdateInstalled {
            program: installed.program.clone(),
        });
    }

    fn offer_changed(&mut self, cx: &mut Context<Self>) {
        self.action = match &self.offer {
            UpdateOffer::Nothing => None,
            UpdateOffer::Available(Update { version, .. }) => {
                Some(format!("Update to {version}").into())
            }
            UpdateOffer::Installing(Update { version, .. }) => {
                Some(format!("Updating to {version}…").into())
            }
            UpdateOffer::Installed(Installed { version, .. }) => {
                Some(format!("Restart for {version}").into())
            }
        };
        cx.notify();
    }
}
