use std::path::Path;

use crate::code_source::CodeSource;
use crate::pull_request::Sha;
use crate::repin::{Repin, repin};
use crate::tour::Tour;

use super::review_state::{RepinRecord, ReviewStateFile};

/// Re-pin as both the CLI and the app run it: the tour moves to the new commits, and the
/// per-PR state learns what became of each piece, so the viewer can show it.
pub fn repin_tour(
    tour_path: &Path,
    state_file: &ReviewStateFile,
    source: &dyn CodeSource,
    new_base: &Sha,
    new_head: &Sha,
) -> anyhow::Result<(Tour, Repin)> {
    let mut tour = Tour::load(tour_path)?;
    let (old_base, old_head) = (Sha::new(&tour.base), Sha::new(&tour.head));
    let result = repin(&mut tour, source, new_base, new_head)?;
    tour.save(tour_path)?;
    let record = RepinRecord::new(&old_base, &old_head, new_base, new_head, &result);
    state_file.update(|state| state.record_repin(record, &result))?;
    Ok((tour, result))
}
