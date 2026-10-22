use anyhow::Context;
use clap::{Parser, Subcommand};

use guided_review::github::GitHub;
use guided_review::local_git::LocalGit;
use guided_review::outline::Outline;
use guided_review::pick_code_source::pick_code_source;
use guided_review::pieces;
use guided_review::pull_request::PullRequestUrl;
use guided_review::show::Show;
use guided_review::tour::Tour;
use guided_review::tour_check;

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
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Outline { pr } => outline(&pr),
        Command::Show { pr, ids, around } => show(&pr, &ids, around),
        Command::Check { pr } => check(&pr),
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
            files: &files
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

    let tips = GitHub::from_env().pull_request_tips(pr)?;
    let current_dir = std::env::current_dir().context("could not read the current folder")?;
    let source = LocalGit::find(&pr.repo, &current_dir)?.with_context(|| {
        format!(
            "no local clone of {} found; run this inside one, or add it under [clones] in ~/.guided-review/config.toml",
            pr.repo
        )
    })?;
    source.require_commits(&[&tips.base_branch_tip, &tips.head])?;
    let base = source.merge_base(&tips.base_branch_tip, &tips.head)?;
    let files = pieces::cut(&source, &base, &tips.head)?;

    tour.pin(pr, &base, &tips.head);
    tour.save(&tour_path)?;

    let report = tour_check::check(&tour, &files);
    print!("{report}");
    if !report.errors.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}
