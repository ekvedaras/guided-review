use crate::pull_request::PullRequestUrl;

/// Which PR the window shows, and what an incoming open request does to it. A PR that is built takes
/// the screen only when it is the newest request, so the one on screen stays until then.
#[derive(Default)]
pub struct ActivePr {
    shown: Option<PullRequestUrl>,
    building: Option<(BuildTicket, PullRequestUrl)>,
    issued: u64,
}

/// Names one build, so its result can be told from the result of an older one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildTicket(u64);

pub enum OpenDecision {
    /// The PR is on screen: read its tour again.
    Reload,
    /// Build the PR's review in the background, then hand the result to `finished`.
    Build(BuildTicket),
}

pub enum Finished<Built> {
    /// The PR now shown is the built one; swap the screen to it and drop the old one.
    Switch(Built),
    /// The build failed; what is on screen stays.
    Stay { reason: String },
    /// A newer request came while this one was building.
    Superseded,
}

impl ActivePr {
    pub fn shown(&self) -> Option<&PullRequestUrl> {
        self.shown.as_ref()
    }

    pub fn request(&mut self, pr: PullRequestUrl) -> OpenDecision {
        self.issued += 1;
        if self.shown.as_ref() == Some(&pr) {
            self.building = None;
            return OpenDecision::Reload;
        }
        let ticket = BuildTicket(self.issued);
        self.building = Some((ticket, pr));
        OpenDecision::Build(ticket)
    }

    pub fn finished<Built>(
        &mut self,
        ticket: BuildTicket,
        built: anyhow::Result<Built>,
    ) -> Finished<Built> {
        let Some((current, pr)) = self.building.take() else {
            return Finished::Superseded;
        };
        if current != ticket {
            self.building = Some((current, pr));
            return Finished::Superseded;
        }
        match built {
            Ok(built) => {
                self.shown = Some(pr);
                Finished::Switch(built)
            }
            Err(error) => Finished::Stay {
                reason: format!("could not open {pr}: {error:#}"),
            },
        }
    }
}
