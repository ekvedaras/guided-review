use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use crate::app::components::palette::Appearance;

use super::drawer::Svg;
use super::strip::Strip;

/// Which diagram is on screen, and how it must look there.
#[derive(Debug, Clone, Copy)]
pub struct Showing<'a> {
    /// The Mermaid text of the step.
    pub source: &'a str,
    pub appearance: Appearance,
    pub scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DiagramId(u64);

impl DiagramId {
    fn of(source: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        Self(hasher.finish())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Shown {
    diagram: DiagramId,
    appearance: Appearance,
    scale: f32,
}

#[derive(Debug, Clone)]
pub struct SvgJob {
    diagram: DiagramId,
    pub source: Arc<str>,
    pub appearance: Appearance,
}

#[derive(Debug, Clone)]
pub struct StripJob {
    shown: Shown,
    pub svg: Svg,
    pub strip: Strip,
    pub scale: f32,
}

/// What the diagrams may hold, so a long tour does not fill memory with pictures.
/// SVG text is small and kept for both appearances, so a theme switch or a step back never waits
/// for merman. Bitmaps are big: only the strips on screen of the shown diagram in the current appearance.
/// Every bitmap the memory lets go is handed back, so the app can free its GPU copy too.
pub struct DiagramMemory<Bitmap> {
    svgs: HashMap<(DiagramId, Appearance), Result<Svg, String>>,
    svgs_in_flight: HashSet<(DiagramId, Appearance)>,
    shown: Option<Shown>,
    pending_svgs: Vec<SvgJob>,
    on_screen: BTreeSet<usize>,
    strips_in_flight: BTreeSet<usize>,
    bitmaps: BTreeMap<usize, Bitmap>,
    /// What to show where a strip of the shown diagram could not be drawn. Drawing it again gives the same result,
    /// so it is not tried again until the diagram, theme or scale changes.
    failed_strips: BTreeMap<usize, Arc<str>>,
}

impl<Bitmap> Default for DiagramMemory<Bitmap> {
    fn default() -> Self {
        Self {
            svgs: HashMap::new(),
            svgs_in_flight: HashSet::new(),
            shown: None,
            pending_svgs: Vec::new(),
            on_screen: BTreeSet::new(),
            strips_in_flight: BTreeSet::new(),
            bitmaps: BTreeMap::new(),
            failed_strips: BTreeMap::new(),
        }
    }
}

impl<Bitmap> DiagramMemory<Bitmap> {
    /// Returns the bitmaps that no longer fit what is shown.
    pub fn show(&mut self, showing: Option<Showing<'_>>) -> Vec<Bitmap> {
        let shown = showing.map(|showing| Shown {
            diagram: DiagramId::of(showing.source),
            appearance: showing.appearance,
            scale: showing.scale,
        });
        if shown == self.shown {
            return Vec::new();
        }
        self.shown = shown;
        self.on_screen.clear();
        self.strips_in_flight.clear();
        self.failed_strips.clear();
        if let Some(showing) = showing {
            self.ask_for_svgs(showing);
        }
        std::mem::take(&mut self.bitmaps).into_values().collect()
    }

    fn ask_for_svgs(&mut self, showing: Showing<'_>) {
        let diagram = DiagramId::of(showing.source);
        let current = showing.appearance;
        let other = match current {
            Appearance::Light => Appearance::Dark,
            Appearance::Dark => Appearance::Light,
        };
        for appearance in [current, other] {
            let key = (diagram, appearance);
            if self.svgs.contains_key(&key) || !self.svgs_in_flight.insert(key) {
                continue;
            }
            self.pending_svgs.push(SvgJob {
                diagram,
                source: showing.source.into(),
                appearance,
            });
        }
    }

    pub fn svgs_to_draw(&mut self) -> Vec<SvgJob> {
        std::mem::take(&mut self.pending_svgs)
    }

    pub fn put_svg(&mut self, job: &SvgJob, svg: Result<Svg, String>) {
        let key = (job.diagram, job.appearance);
        self.svgs_in_flight.remove(&key);
        self.svgs.insert(key, svg);
    }

    fn shown_svg(&self) -> Option<&Result<Svg, String>> {
        let shown = self.shown?;
        self.svgs.get(&(shown.diagram, shown.appearance))
    }

    /// None while merman is still drawing it, or when it failed.
    pub fn svg(&self) -> Option<&Svg> {
        self.shown_svg()?.as_ref().ok()
    }

    pub fn error(&self) -> Option<&str> {
        self.shown_svg()?.as_ref().err().map(String::as_str)
    }

    pub fn strips(&self) -> Vec<Strip> {
        match (self.shown, self.svg()) {
            (Some(shown), Some(svg)) => Strip::cut(svg.height(), shown.scale),
            _ => Vec::new(),
        }
    }

    /// Returns the bitmap of a strip that left the screen.
    pub fn set_on_screen(&mut self, strip: usize, on_screen: bool) -> Option<Bitmap> {
        if on_screen {
            self.on_screen.insert(strip);
            None
        } else {
            self.on_screen.remove(&strip);
            self.strips_in_flight.remove(&strip);
            self.bitmaps.remove(&strip)
        }
    }

    pub fn strips_to_draw(&mut self) -> Vec<StripJob> {
        let (Some(shown), Some(svg)) = (self.shown, self.svg().cloned()) else {
            return Vec::new();
        };
        #[expect(
            clippy::needless_collect,
            reason = "the collect ends the shared borrow of self before the map below changes it"
        )]
        let wanted: Vec<Strip> = self
            .strips()
            .into_iter()
            .filter(|strip| {
                let index = strip.index();
                self.on_screen.contains(&index)
                    && !self.bitmaps.contains_key(&index)
                    && !self.strips_in_flight.contains(&index)
                    && !self.failed_strips.contains_key(&index)
            })
            .collect();
        wanted
            .into_iter()
            .map(|strip| {
                self.strips_in_flight.insert(strip.index());
                StripJob {
                    shown,
                    svg: svg.clone(),
                    strip,
                    scale: shown.scale,
                }
            })
            .collect()
    }

    /// Returns the bitmap when it came too late: the theme, step or scale changed, or the strip scrolled away.
    pub fn put_bitmap(&mut self, job: &StripJob, bitmap: Bitmap) -> Option<Bitmap> {
        let index = job.strip.index();
        if self.shown != Some(job.shown) || !self.strips_in_flight.remove(&index) {
            return Some(bitmap);
        }
        self.bitmaps.insert(index, bitmap)
    }

    /// A failure that came too late is dropped, like a late bitmap.
    pub fn strip_failed(&mut self, job: &StripJob, error: Arc<str>) {
        let index = job.strip.index();
        if self.shown == Some(job.shown) && self.strips_in_flight.remove(&index) {
            self.failed_strips.insert(index, error);
        }
    }

    pub fn strip_failure(&self, strip: usize) -> Option<&Arc<str>> {
        self.failed_strips.get(&strip)
    }

    pub fn bitmap(&self, strip: usize) -> Option<&Bitmap> {
        self.bitmaps.get(&strip)
    }
}
