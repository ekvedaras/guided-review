use anyhow::Context;
use clap::{Parser, Subcommand};

use guided_review::agent_reply::{AgentReply, agent_reply};
use guided_review::app_link::{
    Answer, AppFolder, AppListener, BindError, OpenRequest, Request, hand_over,
    restart as restart_app,
};
use guided_review::claude_cli::{ClaudeEvalWriter, ClaudeTourJudge};
use guided_review::config::Config;
use guided_review::eval::{Eval, EvalStore, Skill, TourSource};
use guided_review::github::GitHub;
use guided_review::opened_pr::{OpenedPr, home_folder};
use guided_review::outline::Outline;
use guided_review::pieces;
use guided_review::pull_request::PullRequestUrl;
use guided_review::show::Show;
use guided_review::summary_instructions::SummaryInstructions;
use guided_review::test_pairing::SubjectCandidates;
use guided_review::timestamp::UtcTimestamp;
use guided_review::tour::{StepId, Tour};
use guided_review::tour_check;
use guided_review::viewer::{HandOffId, RefreshBrief, repin_tour};

#[derive(Parser)]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GUIDED_REVIEW_GIT"), ")"), about)]
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
    /// Quit the running app and open a PR in a new one, as after an install
    Restart {
        pr: PullRequestUrl,
        /// The id of the step to show, else the summary
        #[arg(long)]
        at: Option<StepId>,
    },
    /// Show the app's Settings, also with no PR open
    Settings,
    /// Answer a reviewer's hand-off on its GitHub thread; the reply starts with the robot mark
    AgentReply {
        pr: PullRequestUrl,
        /// The hand-off id from the message, like `h0`
        hand_off: HandOffId,
        /// The answer in GitHub markdown; `-` reads it from stdin, which keeps its line breaks
        #[arg(long, required_unless_present = "working", conflicts_with = "working")]
        body: Option<String>,
        /// Post "Working on it…" now; the answer with --body replaces it
        #[arg(long)]
        working: bool,
    },
    /// Run the app itself; `open` starts it when none answers
    #[command(hide = true)]
    Serve,
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
        Command::Open { pr } => open(&pr, &Launch::Open),
        Command::Restart { pr, at } => open(&pr, &Launch::Restart { at }),
        Command::Settings => settings(),
        Command::AgentReply {
            pr,
            hand_off,
            body,
            working: _,
        } => {
            let body = match body.as_deref() {
                Some("-") => Some(std::io::read_to_string(std::io::stdin())?),
                _ => body,
            };
            let reply = match &body {
                Some(body) => AgentReply::Answer(body.trim_end()),
                None => AgentReply::WorkingOnIt,
            };
            let said = agent_reply(&GitHub::from_env(), &home_folder()?, &pr, hand_off, &reply)?;
            println!("{said}");
            Ok(())
        }
        Command::Serve => serve(),
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

/// A config that cannot be read must not stop `outline` or `check`, which work without it.
fn summary_instructions() -> anyhow::Result<SummaryInstructions> {
    let home = home_folder()?;
    match Config::load_from(&home) {
        Ok(config) => Ok(config.summary_instructions(&home)),
        Err(error) => {
            eprintln!("warning: summary instructions not read: {error:#}");
            Ok(SummaryInstructions::none())
        }
    }
}

fn outline(pr: &PullRequestUrl) -> anyhow::Result<()> {
    let summary_instructions = summary_instructions()?;
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
            summary_instructions: &summary_instructions,
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

    let report = tour_check::check(&tour, &files, &summary_instructions()?);
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

/// How `open` reaches the app: through the one running, or by quitting it first.
enum Launch {
    Open,
    /// Shows the step `at`, else the summary.
    Restart {
        at: Option<StepId>,
    },
}

fn open(pr: &PullRequestUrl, launch: &Launch) -> anyhow::Result<()> {
    let home = home_folder()?;
    let folder = AppFolder::in_home(&home);
    let request = Request::Open(OpenRequest {
        pr: pr.to_string(),
        folder: std::env::current_dir().context("could not read the current folder")?,
        terminal: std::env::var("HC_TERMINAL_ID").ok(),
        at_step: match launch {
            Launch::Open => None,
            Launch::Restart { at } => at.clone(),
        },
    });
    let answer = match launch {
        Launch::Restart { .. } => restart_app(&folder, &request)?,
        Launch::Open => hand_over(&folder, &request)?,
    };
    match answer {
        Answer::Opening { .. }
        | Answer::Writing { .. }
        | Answer::Reloaded { .. }
        | Answer::KeptInBackground { .. } => {
            println!("{answer}");
            Ok(())
        }
        other => anyhow::bail!("{other}"),
    }
}

fn settings() -> anyhow::Result<()> {
    let answer = hand_over(&AppFolder::in_home(&home_folder()?), &Request::ShowSettings)?;
    match answer {
        Answer::ShowingSettings => {
            println!("{answer}");
            Ok(())
        }
        other => anyhow::bail!("{other}"),
    }
}

fn serve() -> anyhow::Result<()> {
    let home = home_folder()?;
    match AppListener::bind(&AppFolder::in_home(&home)) {
        Ok(listener) => {
            guided_review::app::serve(listener, home);
            Ok(())
        }
        Err(BindError::AlreadyRunning) => Ok(()),
        Err(BindError::Failed(error)) => Err(error),
    }
}
