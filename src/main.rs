use anyhow::Context;
use clap::{Parser, Subcommand};

use guided_review::code_source::CodeSource;
use guided_review::config::Config;
use guided_review::github::GitHub;
use guided_review::local_git::LocalGit;
use guided_review::outline::Outline;
use guided_review::pieces;
use guided_review::pull_request::PullRequestUrl;

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
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Outline { pr } => outline(&pr),
    }
}

fn outline(pr: &PullRequestUrl) -> anyhow::Result<()> {
    let tips = GitHub::from_env().pull_request_tips(pr)?;
    let current_dir = std::env::current_dir().context("could not read the current folder")?;
    let source = LocalGit::find(&pr.repo, &current_dir, &Config::load()?)?.with_context(|| {
        format!(
            "no local clone of {} found; run this inside one, or add it under [clones] in ~/.guided-review/config.toml",
            pr.repo
        )
    })?;
    source.require_commits(&[&tips.base_branch, &tips.head])?;
    let base = source.merge_base(&tips.base_branch, &tips.head)?;
    let files = pieces::cut(&source, &base, &tips.head)?;
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
