//! Pictures of the app for looking at the UI without a screen grab. Built with the `snapshots`
//! feature and on when `GUIDED_REVIEW_SNAPSHOTS` names a folder: every screen in reading order is
//! saved there as a PNG, its top and its end, then the app quits.

use std::path::PathBuf;

use gpui::Window;

const ENV: &str = "GUIDED_REVIEW_SNAPSHOTS";
/// Frames to let a screen lay out, load its code and draw its diagram first.
const SETTLE_FRAMES: u32 = 40;
const MAX_SCREENS: usize = 12;

pub struct Snapshots {
    folder: PathBuf,
    frames_on_screen: u32,
    taken: usize,
    is_at_end: bool,
}

pub enum SnapshotMove {
    Wait,
    ScrollToEnd,
    NextScreen,
    Done,
}

impl Snapshots {
    pub fn from_env() -> Option<Self> {
        let folder = PathBuf::from(std::env::var(ENV).ok().filter(|value| !value.is_empty())?);
        std::fs::create_dir_all(&folder).ok()?;
        Some(Self {
            folder,
            frames_on_screen: 0,
            taken: 0,
            is_at_end: false,
        })
    }

    pub fn frame(&mut self, window: &mut Window) -> SnapshotMove {
        window.request_animation_frame();
        self.frames_on_screen += 1;
        if self.frames_on_screen < SETTLE_FRAMES {
            return SnapshotMove::Wait;
        }
        let part = if self.is_at_end { "end" } else { "top" };
        let path = self.folder.join(format!("{:02}-{part}.png", self.taken));
        match window.render_to_image() {
            Ok(image) => {
                if let Err(error) = image.save(&path) {
                    eprintln!("could not save {}: {error}", path.display());
                }
            }
            Err(error) => eprintln!("could not draw the window: {error:#}"),
        }
        self.frames_on_screen = 0;
        if !self.is_at_end {
            self.is_at_end = true;
            return SnapshotMove::ScrollToEnd;
        }
        self.is_at_end = false;
        self.taken += 1;
        if self.taken >= MAX_SCREENS {
            SnapshotMove::Done
        } else {
            SnapshotMove::NextScreen
        }
    }
}
