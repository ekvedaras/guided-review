use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};

use guided_review::app::AskSetup;
use guided_review::claude_cli::{
    ClaudeCli, ClaudeEvalWriter, ClaudeRefreshWriter, ClaudeTourJudge,
};
use guided_review::config::Config;
use guided_review::eval::{Eval, EvalStore, Skill, TourSource};
use guided_review::github::GitHub;
use guided_review::ide::Ide;
use guided_review::outline::Outline;
use guided_review::pick_code_source::pick_code_source;
use guided_review::pieces;
use guided_review::pull_request::{PullRequestTips, PullRequestUrl, Sha};
use guided_review::show::Show;
use guided_review::test_pairing::SubjectCandidates;
use guided_review::tour::Tour;
use guided_review::tour_check;
use guided_review::viewer::{
    Asks, GitHubPort, PickCodeSource, RefreshBrief, Review, ReviewStateFile, TourRefresher,
    TourRepinner, repin_tour,
};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the diff pieces of a PR, grouped by file, without code
    Outline { pr: PullRequestUrl },
    /// Print where the tour file of a PR lives
    TourPath { pr: PullRequestUrl },
    /// Print the diff of the given pieces
    Show {
        pr: PullRequestUrl,
        /// Piece ids as printed by `outline`: `<file>#<symbol>` or `<file>`
        #[arg(required = true)]
        ids: Vec<String>,
        /// Also print each piece's code as it is at head
        #[arg(long)]
        around: bool,
    },
    /// Pin the tour file of a PR to its commits, then list its errors and warnings
    Check { pr: PullRequestUrl },
    /// Move the tour to the PR's new head and print what became of each piece
    Repin { pr: PullRequestUrl },
    /// Print what a Refresh of the tour has to do; `--done` clears the marks once it is done
    Refresh {
        pr: PullRequestUrl,
        #[arg(long)]
        done: bool,
    },
    /// Open the app on the tour of a PR
    Open { pr: PullRequestUrl },
    /// Score the /tour skill on the test set; `eval add` adds a PR to it
    Eval {
        #[command(subcommand)]
        action: Option<EvalAction>,
        /// Judge the tours already on disk instead of writing new ones
        #[arg(long)]
        existing_tours: bool,
    },
}

#[derive(Subcommand)]
enum EvalAction {
    /// Add a PR with the reviewer note on what a good tour of it shows
    Add {
        pr: PullRequestUrl,
        #[arg(long)]
        note: String,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Outline { pr } => outline(&pr),
        Command::Show { pr, ids, around } => show(&pr, &ids, around),
        Command::TourPath { pr } => tour_path(&pr),
        Command::Check { pr } => check(&pr),
        Command::Repin { pr } => repin_command(&pr),
        Command::Refresh { pr, done } => refresh(&pr, done),
        Command::Open { pr } => open(&pr),
        Command::Eval {
            action: Some(EvalAction::Add { pr, note }),
            ..
        } => eval_add(&pr, &note),
        Command::Eval {
            action: None,
            existing_tours,
        } => eval(existing_tours),
    }
}

fn tour_path(pr: &PullRequestUrl) -> anyhow::Result<()> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    println!("{}", Tour::path_for(&home, pr).display());
    Ok(())
}

fn outline(pr: &PullRequestUrl) -> anyhow::Result<()> {
    let github = GitHub::from_env();
    let tips = github.pull_request_tips(pr)?;
    let current_dir = std::env::current_dir().context("could not read the current folder")?;
    let source = pick_code_source(&github, pr, &tips, &current_dir)?;
    let base = source.merge_base(&tips.base_branch_tip, &tips.head)?;
    let files = pieces::cut(source.as_ref(), &base, &tips.head)?;
    print!(
        "{}",
        Outline {
            pr,
            base: &base,
            head: &tips.head,
            files: &files,
            worktree_at_head: source.working_tree_is_at(&tips.head),
            subjects: &SubjectCandidates::find(source.as_ref(), &tips.head, &files),
        }
    );
    Ok(())
}

fn show(pr: &PullRequestUrl, ids: &[String], around: bool) -> anyhow::Result<()> {
    let github = GitHub::from_env();
    let tips = github.pull_request_tips(pr)?;
    let current_dir = std::env::current_dir().context("could not read the current folder")?;
    let source = pick_code_source(&github, pr, &tips, &current_dir)?;
    let base = source.merge_base(&tips.base_branch_tip, &tips.head)?;
    let files = pieces::cut(source.as_ref(), &base, &tips.head)?;
    print!(
        "{}",
        Show {
            source: source.as_ref(),
            head: &tips.head,
            files: &files,
            ids,
            around
        }
        .render()?
    );
    Ok(())
}

fn check(pr: &PullRequestUrl) -> anyhow::Result<()> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    let tour_path = Tour::path_for(&home, pr);
    let mut tour = Tour::load(&tour_path)?;

    let github = GitHub::from_env();
    let tips = github.pull_request_tips(pr)?;
    let current_dir = std::env::current_dir().context("could not read the current folder")?;
    let source = pick_code_source(&github, pr, &tips, &current_dir)?;
    if tour.head.is_empty() {
        let base = source.merge_base(&tips.base_branch_tip, &tips.head)?;
        tour.pin(pr, &base, &tips.head);
    } else {
        if tour.pr.is_empty() {
            tour.pr = pr.to_string();
        }
        if tour.head != tips.head.to_string() {
            println!("warning: PR has new commits; run `guided-review repin`");
        }
    }
    tour.save(&tour_path)?;
    let files = pieces::cut(
        source.as_ref(),
        &Sha::new(&tour.base),
        &Sha::new(&tour.head),
    )?;

    let report = tour_check::check(&tour, &files);
    print!("{report}");
    if !report.errors.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

fn repin_command(pr: &PullRequestUrl) -> anyhow::Result<()> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    let tour_path = Tour::path_for(&home, pr);
    // Fails before any network call when there is no tour yet.
    Tour::load(&tour_path)?;

    let github = GitHub::from_env();
    let tips = github.pull_request_tips(pr)?;
    let current_dir = std::env::current_dir().context("could not read the current folder")?;
    let source = pick_code_source(&github, pr, &tips, &current_dir)?;
    let base = source.merge_base(&tips.base_branch_tip, &tips.head)?;

    let state_file = ReviewStateFile::for_pr(&home, pr);
    let (_, result) = repin_tour(&tour_path, &state_file, source.as_ref(), &base, &tips.head)?;
    print!("{result}");
    Ok(())
}

fn refresh(pr: &PullRequestUrl, done: bool) -> anyhow::Result<()> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    let state_file = ReviewStateFile::for_pr(&home, pr);
    if done {
        state_file.update(|state| state.finish_refresh())?;
        println!("refresh done: marks cleared");
        return Ok(());
    }
    let tour = Tour::load(&Tour::path_for(&home, pr))?;
    let pinned = PullRequestTips {
        base_branch_tip: Sha::new(&tour.base),
        head: Sha::new(&tour.head),
    };
    let current_dir = std::env::current_dir().context("could not read the current folder")?;
    let source = pick_code_source(&GitHub::from_env(), pr, &pinned, &current_dir)?;
    let files = pieces::cut(
        source.as_ref(),
        &Sha::new(&tour.base),
        &Sha::new(&tour.head),
    )?;
    print!(
        "{}",
        RefreshBrief::of(&tour, &files, state_file.load()?.last_repin())
    );
    Ok(())
}

fn eval_add(pr: &PullRequestUrl, note: &str) -> anyhow::Result<()> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    EvalStore::in_home(&home).add(pr, note)?;
    println!("added {pr} to the test set");
    Ok(())
}

fn eval(existing_tours: bool) -> anyhow::Result<()> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    let report = Eval {
        store: &EvalStore::in_home(&home),
        home: &home,
        writer: &ClaudeEvalWriter::new(),
        judge: &ClaudeTourJudge::new(),
        skill: &Skill::bundled(),
    }
    .run(
        if existing_tours {
            TourSource::ExistingTours
        } else {
            TourSource::WriteNew
        },
        &utc_timestamp(),
    )?;
    print!("{report}");
    if report.has_failures() {
        std::process::exit(1);
    }
    Ok(())
}

fn utc_timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (days, time) = ((seconds / 86_400) as i64, seconds % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}-{:02}-{:02}",
        time / 3600,
        time % 3600 / 60,
        time % 60
    )
}

fn open(pr: &PullRequestUrl) -> anyhow::Result<()> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    let tour_path = Tour::path_for(&home, pr);
    if !tour_path.exists() {
        anyhow::bail!("no tour for {pr} yet; run `/tour {pr}` first");
    }
    let tour = Tour::load(&tour_path)?;

    // The tour is read at the commits it is pinned to, even when the PR has moved on since.
    let pinned = PullRequestTips {
        base_branch_tip: Sha::new(&tour.base),
        head: Sha::new(&tour.head),
    };
    let current_dir = std::env::current_dir().context("could not read the current folder")?;
    let github = GitHub::from_env();
    let source = pick_code_source(&github, pr, &pinned, &current_dir)?;
    let review = Review::open(tour, source.as_ref(), ReviewStateFile::for_pr(&home, pr))?;

    let clone = source.local_clone().map(Path::to_path_buf);
    let config = Config::load()?;
    let pick_code_source: PickCodeSource = {
        let (github, pr, current_dir) = (github.clone(), pr.clone(), current_dir.clone());
        Arc::new(move |tips| pick_code_source(&github, &pr, tips, &current_dir))
    };

    let ask = AskSetup {
        asks: Asks::load(Arc::new(ReviewStateFile::for_pr(&home, pr)))?,
        launcher: Arc::new(ClaudeCli::new()),
        clone: clone.clone(),
        settings: config.ask_settings(),
    };
    let refresher = TourRefresher {
        home: home.clone(),
        pr: pr.clone(),
        pick_code_source: pick_code_source.clone(),
        writer: Arc::new(ClaudeRefreshWriter::new(
            clone.clone().unwrap_or_else(|| home.clone()),
        )),
    };
    let repinner = TourRepinner {
        home,
        pr: pr.clone(),
        pick_code_source,
    };
    let ide = Ide::for_clone(clone, config.ide_template());
    guided_review::app::run(
        review,
        source.into(),
        ask,
        ide,
        Arc::new(github),
        repinner,
        refresher,
    );
    Ok(())
}
