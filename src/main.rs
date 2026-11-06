use anyhow::Context;
use clap::{Parser, Subcommand};

use guided_review::claude_cli::{ClaudeEvalWriter, ClaudeTourJudge};
use guided_review::eval::{Eval, EvalStore, Skill, TourSource};
use guided_review::open_review::open_review;
use guided_review::opened_pr::{OpenedPr, home_folder};
use guided_review::outline::Outline;
use guided_review::pieces;
use guided_review::pull_request::PullRequestUrl;
use guided_review::show::Show;
use guided_review::test_pairing::SubjectCandidates;
use guided_review::timestamp::UtcTimestamp;
use guided_review::tour::Tour;
use guided_review::tour_check;
use guided_review::viewer::{RefreshBrief, repin_tour};

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
    println!("{}", Tour::path_for(&home_folder()?, pr).display());
    Ok(())
}

fn outline(pr: &PullRequestUrl) -> anyhow::Result<()> {
    let code = OpenedPr::new(pr)?.at_live_tips()?;
    let (base, files) = code.pieces_since_merge_base()?;
    print!(
        "{}",
        Outline {
            pr,
            base: &base,
            head: &code.tips.head,
            files: &files,
            worktree_at_head: code.source.working_tree_is_at(&code.tips.head),
            subjects: &SubjectCandidates::find(code.source.as_ref(), &code.tips.head, &files),
        }
    );
    Ok(())
}

fn show(pr: &PullRequestUrl, ids: &[String], around: bool) -> anyhow::Result<()> {
    let code = OpenedPr::new(pr)?.at_live_tips()?;
    let (_, files) = code.pieces_since_merge_base()?;
    print!(
        "{}",
        Show {
            source: code.source.as_ref(),
            head: &code.tips.head,
            files: &files,
            ids,
            around
        }
        .render()?
    );
    Ok(())
}

fn check(pr: &PullRequestUrl) -> anyhow::Result<()> {
    let opened = OpenedPr::new(pr)?;
    let tour_path = opened.tour_path();
    let mut tour = Tour::load(&tour_path)?;

    let code = opened.at_live_tips()?;
    if tour.head.is_empty() {
        tour.pin(pr, &code.merge_base()?, &code.tips.head);
    } else {
        if tour.pr.is_empty() {
            tour.pr = pr.to_string();
        }
        if tour.head != code.tips.head {
            println!("warning: PR has new commits; run `guided-review repin`");
        }
    }
    tour.save(&tour_path)?;
    let files = pieces::cut(code.source.as_ref(), &tour.base, &tour.head)?;

    let report = tour_check::check(&tour, &files);
    print!("{report}");
    if !report.errors.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

fn repin_command(pr: &PullRequestUrl) -> anyhow::Result<()> {
    let opened = OpenedPr::new(pr)?;
    let tour_path = opened.tour_path();
    // Fails before any network call when there is no tour yet.
    Tour::load(&tour_path)?;

    let code = opened.at_live_tips()?;
    let base = code.merge_base()?;
    let (_, result) = repin_tour(
        &tour_path,
        &opened.state_file(),
        code.source.as_ref(),
        &base,
        &code.tips.head,
    )?;
    print!("{result}");
    Ok(())
}

fn refresh(pr: &PullRequestUrl, done: bool) -> anyhow::Result<()> {
    let opened = OpenedPr::new(pr)?;
    let state_file = opened.state_file();
    if done {
        state_file.update(guided_review::viewer::ReviewState::finish_refresh)?;
        println!("refresh done: marks cleared");
        return Ok(());
    }
    let tour = Tour::load(&opened.tour_path())?;
    let code = opened.at_pinned_tips(&tour)?;
    let files = pieces::cut(code.source.as_ref(), &tour.base, &tour.head)?;
    print!(
        "{}",
        RefreshBrief::of(&tour, &files, state_file.load()?.last_repin())
    );
    Ok(())
}

fn eval_add(pr: &PullRequestUrl, note: &str) -> anyhow::Result<()> {
    let home = home_folder()?;
    EvalStore::in_home(&home).add(pr, note)?;
    println!("added {pr} to the test set");
    Ok(())
}

fn eval(existing_tours: bool) -> anyhow::Result<()> {
    let home = home_folder()?;
    let report = Eval {
        store: &EvalStore::in_home(&home),
        home: &home,
        writer: &ClaudeEvalWriter::new(),
        judge: &ClaudeTourJudge::new(),
        skill: &Skill::bundled(),
    }
    .run(
        if existing_tours {
            &TourSource::ExistingTours
        } else {
            &TourSource::WriteNew
        },
        &UtcTimestamp::now().to_string(),
    )?;
    print!("{report}");
    if report.has_failures() {
        std::process::exit(1);
    }
    Ok(())
}

fn open(pr: &PullRequestUrl) -> anyhow::Result<()> {
    let current_dir = std::env::current_dir().context("could not read the current folder")?;
    guided_review::app::run(open_review(pr, &current_dir, &home_folder()?)?);
    Ok(())
}
