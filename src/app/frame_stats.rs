//! Frame-time measuring for the big-PR speed bar (60 fps). Off unless `GUIDED_REVIEW_FRAME_STATS` is set:
//! `1` logs frame stats to stderr every few seconds and on quit, `scroll` runs the scroll script
//! (every screen in reading order, top to bottom at a fixed speed, stats per screen, then quit).

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{IntoElement, ListState, Pixels, Styled, Window, canvas, px};

const ENV: &str = "GUIDED_REVIEW_FRAME_STATS";
const LOG_EVERY: Duration = Duration::from_secs(5);
const FRAME_BUDGET: Duration = Duration::from_micros(16_667);
/// An interval this long means at least one 60 Hz frame was skipped; vsync jitter stays below it.
const DROPPED_FRAME_INTERVAL: Duration = Duration::from_millis(25);
const SCROLL_SPEED_PER_FRAME: Pixels = px(40.);
/// Frames to let a new screen lay out, draw its first diagram strips and settle before the measured scroll.
const SETTLE_FRAMES: u32 = 20;
/// Caps a screen's run, so the slow baseline of a huge step still ends in minutes.
const MAX_SCROLL_FRAMES: u32 = 1500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameStatsMode {
    Log,
    ScrollScript,
}

impl FrameStatsMode {
    pub fn from_env() -> Option<Self> {
        match std::env::var(ENV).ok()?.as_str() {
            "" | "0" => None,
            "scroll" => Some(Self::ScrollScript),
            _ => Some(Self::Log),
        }
    }
}

/// Frame work is the time from the root view's render to the last element painted; the interval is
/// the time between two frame starts, so it also shows frames the platform skipped.
#[derive(Default)]
struct FrameClock {
    frame_started_at: Option<Instant>,
    work: Vec<Duration>,
    intervals: Vec<Duration>,
}

impl FrameClock {
    fn frame_started(&mut self) {
        let now = Instant::now();
        if let Some(previous) = self.frame_started_at.replace(now) {
            self.intervals.push(now - previous);
        }
    }

    fn frame_painted(&mut self) {
        if let Some(started) = self.frame_started_at {
            self.work.push(started.elapsed());
        }
    }

    fn take_report(&mut self) -> FrameReport {
        FrameReport {
            work: Percentiles::of(std::mem::take(&mut self.work)),
            intervals: Percentiles::of(std::mem::take(&mut self.intervals)),
        }
    }
}

pub struct FrameReport {
    work: Percentiles,
    intervals: Percentiles,
}

impl fmt::Display for FrameReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} frames | work p50 {} p90 {} p99 {} max {} | interval p50 {} p90 {} p99 {} max {} | work over 16.7ms {}, intervals over 25ms {}",
            self.work.count,
            ms(self.work.p50),
            ms(self.work.p90),
            ms(self.work.p99),
            ms(self.work.max),
            ms(self.intervals.p50),
            ms(self.intervals.p90),
            ms(self.intervals.p99),
            ms(self.intervals.max),
            self.work.over(FRAME_BUDGET),
            self.intervals.over(DROPPED_FRAME_INTERVAL),
        )
    }
}

#[derive(Default)]
struct Percentiles {
    count: usize,
    p50: Duration,
    p90: Duration,
    p99: Duration,
    max: Duration,
    samples: Vec<Duration>,
}

impl Percentiles {
    fn of(mut samples: Vec<Duration>) -> Self {
        samples.sort();
        let Some(&max) = samples.last() else {
            return Self::default();
        };
        let at = |per_mille: usize| {
            let rank = ((samples.len() - 1) * per_mille + 500) / 1000;
            samples.get(rank).copied().unwrap_or(max)
        };
        Self {
            count: samples.len(),
            p50: at(500),
            p90: at(900),
            p99: at(990),
            max,
            samples,
        }
    }

    fn over(&self, limit: Duration) -> usize {
        self.samples
            .iter()
            .filter(|sample| **sample > limit)
            .count()
    }
}

fn ms(duration: Duration) -> String {
    format!("{:.1}ms", duration.as_secs_f64() * 1000.)
}

struct FrameStats {
    mode: FrameStatsMode,
    clock: Rc<RefCell<FrameClock>>,
    last_log: Instant,
    script: ScrollScript,
}

impl FrameStats {
    pub fn new(mode: FrameStatsMode) -> Self {
        Self {
            mode,
            clock: Rc::default(),
            last_log: Instant::now(),
            script: ScrollScript::default(),
        }
    }

    pub fn is_scroll_script(&self) -> bool {
        self.mode == FrameStatsMode::ScrollScript
    }

    /// Call at the start of the root render; put the returned marker last in the tree, so it paints last.
    pub fn frame_started(&mut self) -> impl IntoElement + use<> {
        self.clock.borrow_mut().frame_started();
        if self.mode == FrameStatsMode::Log && self.last_log.elapsed() >= LOG_EVERY {
            self.last_log = Instant::now();
            eprintln!("frame stats: {}", self.clock.borrow_mut().take_report());
        }
        let clock = self.clock.clone();
        canvas(
            |_, _, _| {},
            move |_, _, _, _| clock.borrow_mut().frame_painted(),
        )
        .absolute()
        .size_0()
    }

    pub fn report_on_quit(&self) {
        if self.mode == FrameStatsMode::Log {
            eprintln!("frame stats: {}", self.clock.borrow_mut().take_report());
        }
    }

    pub fn code_arrived(&mut self) {
        self.script.code_arrived = true;
    }

    /// One frame of the scroll script. Returns what the layout must do next.
    /// The window is raised at each screen, since gpui throttles an inactive window to 30 fps.
    pub fn scroll_script_frame(
        &mut self,
        screen: &dyn ScrollableScreen,
        window: &Window,
    ) -> ScriptMove {
        let script = &mut self.script;
        if !script.code_arrived {
            return ScriptMove::Wait;
        }
        if script.settle_frames < SETTLE_FRAMES {
            if script.settle_frames == 0 {
                window.activate_window();
            }
            script.settle_frames += 1;
            if script.settle_frames == SETTLE_FRAMES {
                self.clock.borrow_mut().take_report();
            }
            return ScriptMove::Wait;
        }
        let scrolled = screen.scrolled();
        let has_moved = script.last_scrolled != Some(scrolled);
        if has_moved && script.scroll_frames < MAX_SCROLL_FRAMES {
            script.last_scrolled = Some(scrolled);
            screen.scroll_down(SCROLL_SPEED_PER_FRAME);
            script.scroll_frames += 1;
            return ScriptMove::Wait;
        }
        script.last_scrolled = None;
        script.settle_frames = 0;
        script.scroll_frames = 0;
        ScriptMove::ScreenDone(self.clock.borrow_mut().take_report())
    }
}

#[derive(Default)]
struct ScrollScript {
    code_arrived: bool,
    settle_frames: u32,
    scroll_frames: u32,
    last_scrolled: Option<Pixels>,
}

pub enum ScriptMove {
    Wait,
    ScreenDone(FrameReport),
}

/// The scroll script's handle on whatever scrolls the screen.
pub trait ScrollableScreen {
    fn scroll_down(&self, by: Pixels);
    /// How far down it is; the script stops when a scroll step no longer moves it.
    fn scrolled(&self) -> Pixels;
}

impl ScrollableScreen for ListState {
    fn scroll_down(&self, by: Pixels) {
        self.scroll_by(by);
    }

    fn scrolled(&self) -> Pixels {
        -self.scroll_px_offset_for_scrollbar().y
    }
}

/// The frame stats as the session and the layout reach them; every call does nothing when they are off.
#[derive(Clone, Default)]
pub struct FrameStatsHook(Option<Rc<RefCell<FrameStats>>>);

impl FrameStatsHook {
    pub fn from_env() -> Self {
        Self(FrameStatsMode::from_env().map(|mode| Rc::new(RefCell::new(FrameStats::new(mode)))))
    }

    pub fn rows_rebuilt(&self, rows: usize, took: Duration) {
        if self.0.is_some() {
            eprintln!(
                "review changed: {rows} rows rebuilt in {:.1}ms",
                took.as_secs_f64() * 1000.
            );
        }
    }

    pub fn code_arrived(&self) {
        if let Some(stats) = &self.0 {
            stats.borrow_mut().code_arrived();
        }
    }

    /// Call at the start of the root render; put the returned marker last in the tree, so it paints last.
    pub fn frame_started(&self) -> Option<impl IntoElement + use<>> {
        self.0
            .as_ref()
            .map(|stats| stats.borrow_mut().frame_started())
    }

    pub fn report_on_quit(&self) {
        if let Some(stats) = &self.0 {
            stats.borrow().report_on_quit();
        }
    }

    /// `None` unless the scroll script runs.
    pub fn scroll_script_frame(
        &self,
        screen: &dyn ScrollableScreen,
        window: &Window,
    ) -> Option<ScriptMove> {
        let stats = self.0.as_ref()?;
        let mut stats = stats.borrow_mut();
        if !stats.is_scroll_script() {
            return None;
        }
        Some(stats.scroll_script_frame(screen, window))
    }
}
