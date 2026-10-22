//! Frame-time measuring for the big-PR speed bar (60 fps). Off unless `GUIDED_REVIEW_FRAME_STATS` is set:
//! `1` logs frame stats to stderr every few seconds and on quit, `scroll` runs the scroll script
//! (every screen in reading order, top to bottom at a fixed speed, stats per screen, then quit).

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{IntoElement, Pixels, ScrollHandle, Styled, canvas, point, px};

pub const ENV: &str = "GUIDED_REVIEW_FRAME_STATS";
const LOG_EVERY: Duration = Duration::from_secs(5);
const FRAME_BUDGET: Duration = Duration::from_micros(16_667);
pub const SCROLL_SPEED_PER_FRAME: Pixels = px(40.);
/// Frames to let a new screen lay out, draw its first diagram strips and settle before the measured scroll.
const SETTLE_FRAMES: u32 = 20;
/// Caps a screen's run, so the slow baseline of a huge step still ends in minutes.
const MAX_SCROLL_FRAMES: u32 = 1500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameStatsMode {
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
            "{} frames | work p50 {} p90 {} p99 {} max {} | interval p50 {} p90 {} p99 {} max {} | over 16.7 ms: work {}, interval {}",
            self.work.count,
            ms(self.work.p50),
            ms(self.work.p90),
            ms(self.work.p99),
            ms(self.work.max),
            ms(self.intervals.p50),
            ms(self.intervals.p90),
            ms(self.intervals.p99),
            ms(self.intervals.max),
            self.work.over_budget,
            self.intervals.over_budget,
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
    over_budget: usize,
}

impl Percentiles {
    fn of(mut samples: Vec<Duration>) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        samples.sort();
        let at = |p: f64| samples[((samples.len() - 1) as f64 * p).round() as usize];
        Self {
            count: samples.len(),
            p50: at(0.5),
            p90: at(0.9),
            p99: at(0.99),
            max: *samples.last().expect("not empty"),
            over_budget: samples.iter().filter(|d| **d > FRAME_BUDGET).count(),
        }
    }
}

fn ms(duration: Duration) -> String {
    format!("{:.1}ms", duration.as_secs_f64() * 1000.)
}

pub struct FrameStats {
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
    pub fn scroll_script_frame(&mut self, screen: &dyn ScrollableScreen) -> ScriptMove {
        let script = &mut self.script;
        if !script.code_arrived {
            return ScriptMove::Wait;
        }
        if script.settle_frames < SETTLE_FRAMES {
            script.settle_frames += 1;
            if script.settle_frames == SETTLE_FRAMES {
                self.clock.borrow_mut().take_report();
            }
            return ScriptMove::Wait;
        }
        if !screen.is_at_end() && script.scroll_frames < MAX_SCROLL_FRAMES {
            screen.scroll_down(SCROLL_SPEED_PER_FRAME);
            script.scroll_frames += 1;
            return ScriptMove::Wait;
        }
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
}

pub enum ScriptMove {
    Wait,
    ScreenDone(FrameReport),
}

/// The scroll script's handle on whatever scrolls the screen.
pub trait ScrollableScreen {
    fn scroll_down(&self, by: Pixels);
    fn is_at_end(&self) -> bool;
}

impl ScrollableScreen for ScrollHandle {
    fn scroll_down(&self, by: Pixels) {
        let offset = self.offset();
        let lowest = -self.max_offset().y;
        self.set_offset(point(offset.x, (offset.y - by).max(lowest)));
    }

    fn is_at_end(&self) -> bool {
        self.offset().y <= -self.max_offset().y
    }
}
