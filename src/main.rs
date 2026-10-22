use anyhow::Context;
use clap::{Parser, Subcommand};

use guided_review::github::GitHub;
use guided_review::outline::Outline;
use guided_review::pick_code_source::pick_code_source;
use guided_review::pieces;
use guided_review::pull_request::PullRequestUrl;
use guided_review::show::Show;

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
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Outline { pr } => outline(&pr),
        Command::Show { pr, ids, around } => show(&pr, &ids, around),
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
