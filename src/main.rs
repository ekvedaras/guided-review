use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};

use guided_review::app::AskSetup;
use guided_review::claude_cli::ClaudeCli;
use guided_review::config::Config;
use guided_review::github::GitHub;
use guided_review::local_git::LocalGit;
use guided_review::outline::Outline;
use guided_review::pick_code_source::pick_code_source;
use guided_review::pieces;
use guided_review::pull_request::{PullRequestTips, PullRequestUrl, Sha};
use guided_review::show::Show;
use guided_review::test_pairing::SubjectCandidates;
use guided_review::tour::Tour;
use guided_review::tour_check;
use guided_review::viewer::{Asks, Review, ReviewStateFile, repin_tour};

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
    /// Open the app on the tour of a PR
    Open { pr: PullRequestUrl },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Outline { pr } => outline(&pr),
        Command::Show { pr, ids, around } => show(&pr, &ids, around),
        Command::Check { pr } => check(&pr),
        Command::Repin { pr } => repin_command(&pr),
        Command::Open { pr } => open(&pr),
    }
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

    let ask = AskSetup {
        asks: Asks::load(Arc::new(ReviewStateFile::for_pr(&home, pr)))?,
        launcher: Arc::new(ClaudeCli::new()),
        clone: LocalGit::find(&pr.repo, &current_dir)?.map(|clone| clone.root().to_path_buf()),
        settings: Config::load()?.ask_settings(),
    };
    guided_review::app::run(review, source.into(), ask, Arc::new(github));
    Ok(())
}
