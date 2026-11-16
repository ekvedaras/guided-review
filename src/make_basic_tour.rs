use anyhow::Context;

use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::pull_request::{ChangeTips, PullRequestUrl};
use crate::tour::{BasicTour, Tour, TourPin};

/// Makes the PR's basic tour from the code at `tips` and the PR's own title, and writes it beside the
/// guided tour's path. It reads nothing from GitHub: the caller gives the title.
///
/// Refused while a guided tour is on disk: a guided tour never turns back into a basic one, and a PR
/// with one opens it.
pub fn make_basic_tour(
    folders: &AppFolders,
    pr: &PullRequestUrl,
    source: &dyn CodeSource,
    tips: &ChangeTips,
    pr_title: &str,
) -> anyhow::Result<BasicTour> {
    let guided_path = Tour::path_for(folders, pr);
    anyhow::ensure!(
        !guided_path.exists(),
        "{pr} has a guided tour at {}, so no basic tour is made",
        guided_path.display()
    );
    let base = source
        .merge_base(&tips.base_branch_tip, &tips.head)
        .with_context(|| format!("could not find where {pr} branched off"))?;
    let mut pin: TourPin = serde_json::from_value(serde_json::json!({}))?;
    pin.pin_to(pr, &base, &tips.head);
    let tour = BasicTour::new(pin, pr_title.to_owned());
    let path = Tour::basic_path_for(folders, pr);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)
            .with_context(|| format!("could not create {}", folder.display()))?;
    }
    tour.save(&path)?;
    Ok(tour)
}
