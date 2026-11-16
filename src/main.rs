use anyhow::Context;
use clap::{Parser, Subcommand};

use guided_review::agent_reply::{AgentReply, agent_reply};
use guided_review::app_folders::AppFolders;
use guided_review::app_link::{
    Answer, AppFolder, AppListener, BindError, Caller, OpenRequest, Request, hand_over,
    restart as restart_app,
};
use guided_review::claude_cli::{ClaudeEvalWriter, ClaudeTourJudge};
use guided_review::config::Config;
use guided_review::eval::{Eval, EvalStore, Skill, TourSource};
use guided_review::github::GitHub;
use guided_review::opened_pr::OpenedPr;
use guided_review::outline::Outline;
use guided_review::pieces;
use guided_review::platform::{EntryPlaced, IN_LOGIN_SHELL, write_desktop_entry_templates};
use guided_review::pull_request::PullRequestUrl;
use guided_review::review_subject::ReviewSubject;
use guided_review::show::Show;
use guided_review::summary_instructions::SummaryInstructions;
use guided_review::test_pairing::SubjectCandidates;
use guided_review::timestamp::UtcTimestamp;
use guided_review::tour::{GuidedTour, StepId, Tour, ToursOnDisk};
use guided_review::tour_check;
use guided_review::update::{AppVersion, ReleaseInstall, Version};
use guided_review::viewer::{HandOffId, RefreshBrief, repin_tour};
use std::os::unix::process::CommandExt;

#[derive(Parser)]
#[command(version = AppVersion::RUNNING.as_str(), about)]
struct Cli {
    /// With none, as when the app is started from the OS launcher, the app shows the PR switcher
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Print the diff pieces of a PR, grouped by file, without code
    Outline { subject: ReviewSubject },
    /// Print where the tour file of a PR lives
    TourPath { subject: ReviewSubject },
    /// Print the diff of the given pieces
    Show {
        subject: ReviewSubject,
        /// Piece ids as printed by `outline`: `<file>#<symbol>` or `<file>`
        #[arg(required = true)]
        ids: Vec<String>,
        /// Also print each piece's code as it is at head
        #[arg(long)]
        around: bool,
    },
    /// Pin the tour file of a PR to its commits, then list its errors and warnings
    Check { subject: ReviewSubject },
    /// Move the tour to the PR's new head and print what became of each piece
    Repin { subject: ReviewSubject },
    /// Print what a Refresh of the tour has to do; `--done` clears the marks once it is done
    Refresh {
        subject: ReviewSubject,
        #[arg(long)]
        done: bool,
    },
    /// Open the app on the tour of a PR
    Open { subject: ReviewSubject },
    /// Quit the running app of this build and open a PR in a new one
    Restart {
        subject: ReviewSubject,
        /// The id of the step to show, else the summary
        #[arg(long)]
        at: Option<StepId>,
        /// The build of the app whose Restart ran this, which this may quit though it is another build
        #[arg(long, hide = true)]
        replacing: Option<String>,
    },
    /// Show the app's Settings, also with no PR open
    Settings,
    /// Put the app in the OS launcher (Guided Review.app on macOS), from this release install
    InstallApp,
    /// Write the files the release archives' Desktop entries are made of, for every OS
    #[command(hide = true)]
    DesktopEntryTemplate { out: std::path::PathBuf },
    /// Answer a reviewer's hand-off on its GitHub thread; the reply starts with the robot mark
    AgentReply {
        subject: ReviewSubject,
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
    let cli = Cli::parse();
    AppFolders::settle()?;
    let Some(command) = cli.command else {
        return launch();
    };
    match command {
        Command::Outline { subject } => outline(&pull_request_of(&subject)?),
        Command::Show {
            subject,
            ids,
            around,
        } => show(&pull_request_of(&subject)?, &ids, around),
        Command::TourPath { subject } => tour_path(&pull_request_of(&subject)?),
        Command::Check { subject } => check(&pull_request_of(&subject)?),
        Command::Repin { subject } => repin_command(&pull_request_of(&subject)?),
        Command::Refresh { subject, done } => refresh(&pull_request_of(&subject)?, done),
        Command::Open { subject } => open(&pull_request_of(&subject)?, &Launch::Open),
        Command::Restart {
            subject,
            at,
            replacing,
        } => open(
            &pull_request_of(&subject)?,
            &Launch::Restart { at, replacing },
        ),
        Command::Settings => settings(),
        Command::InstallApp => install_app(),
        Command::DesktopEntryTemplate { out } => {
            write_desktop_entry_templates(&out, Version::running())
        }
        Command::AgentReply {
            subject,
            hand_off,
            body,
            working: _,
        } => {
            let pr = pull_request_of(&subject)?;
            let body = match body.as_deref() {
                Some("-") => Some(std::io::read_to_string(std::io::stdin())?),
                _ => body,
            };
            let reply = match &body {
                Some(body) => AgentReply::Answer(body.trim_end()),
                None => AgentReply::WorkingOnIt,
            };
            let said = agent_reply(
                &GitHub::from_env(),
                &AppFolders::running()?,
                &pr,
                hand_off,
                &reply,
            )?;
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

/// Until a local review is built, every command that takes a subject works on a PR only.
fn pull_request_of(subject: &ReviewSubject) -> anyhow::Result<PullRequestUrl> {
    match subject {
        ReviewSubject::PullRequest(pr) => Ok(pr.clone()),
        ReviewSubject::Local(_) => {
            anyhow::bail!("{subject} is a local change set: local review is not built yet")
        }
    }
}

fn tour_path(pr: &PullRequestUrl) -> anyhow::Result<()> {
    println!(
        "{}",
        Tour::path_for(&AppFolders::running()?, &ReviewSubject::from(pr.clone())).display()
    );
    Ok(())
}

/// A config that cannot be read must not stop `outline` or `check`, which work without it.
fn summary_instructions() -> anyhow::Result<SummaryInstructions> {
    let folders = AppFolders::running()?;
    match Config::load_from(&folders) {
        Ok(config) => Ok(config.summary_instructions(&folders)),
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
    let mut tour = GuidedTour::load(&tour_path)?;

    let code = opened.at_live_tips()?;
    let subject = ReviewSubject::from(pr.clone());
    if tour.pin.head.is_empty() {
        tour.pin
            .pin_to(&subject, &code.merge_base()?, &code.tips.head);
    } else {
        if tour.pin.pr.is_empty() && tour.pin.local.is_none() {
            tour.pin.pr = subject.to_string();
        }
        if tour.pin.head != code.tips.head {
            println!("warning: PR has new commits; run `guided-review repin`");
        }
    }
    tour.save(&tour_path)?;
    let files = pieces::cut(code.source.as_ref(), &tour.pin.base, &tour.pin.head)?;

    let report = tour_check::check(&tour, &files, &summary_instructions()?);
    print!("{report}");
    if !report.errors.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

fn repin_command(pr: &PullRequestUrl) -> anyhow::Result<()> {
    let opened = OpenedPr::new(pr)?;
    let tour_path = opened.tour_to_repin_path();
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
    let tour = GuidedTour::load(&opened.tour_path())?;
    let code = opened.at_pinned_tips(&tour.pin)?;
    let files = pieces::cut(code.source.as_ref(), &tour.pin.base, &tour.pin.head)?;
    print!(
        "{}",
        RefreshBrief::of(&tour, &files, state_file.load()?.last_repin())
    );
    Ok(())
}

fn eval_add(pr: &PullRequestUrl, note: &str) -> anyhow::Result<()> {
    let folders = AppFolders::running()?;
    EvalStore::in_folders(&folders).add(pr, note)?;
    println!("added {pr} to the test set");
    Ok(())
}

fn eval(existing_tours: bool) -> anyhow::Result<()> {
    let folders = AppFolders::running()?;
    let report = Eval {
        store: &EvalStore::in_folders(&folders),
        folders: &folders,
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
        /// Set by the app's Restart: the app's own build, which this may quit.
        replacing: Option<String>,
    },
}

fn open(pr: &PullRequestUrl, launch: &Launch) -> anyhow::Result<()> {
    let folders = &AppFolders::running()?;
    let tours = ToursOnDisk::of(folders, &ReviewSubject::from(pr.clone()));
    if let Launch::Restart { at: Some(step), .. } = launch
        && tours.opens_basic()
    {
        anyhow::bail!(
            "{pr} opens in Basic mode, which has no steps, so there is no step `{step}` to restart at"
        );
    }
    if tours == ToursOnDisk::None {
        OpenedPr::new(pr)?.make_basic_tour_if_no_tour()?;
    }
    let folder = AppFolder::of(folders);
    let request = Request::Open(OpenRequest {
        subject: pr.to_string(),
        folder: std::env::current_dir().context("could not read the current folder")?,
        terminal: std::env::var("HC_TERMINAL_ID").ok(),
        at_step: match launch {
            Launch::Open => None,
            Launch::Restart { at, .. } => at.clone(),
        },
    });
    let answer = match launch {
        Launch::Restart {
            replacing: Some(replacing),
            ..
        } => restart_app(
            &folder,
            &request,
            &Caller::AppRestart {
                replacing: replacing.clone(),
            },
        )?,
        Launch::Restart {
            replacing: None, ..
        } => restart_app(&folder, &request, &Caller::Command)?,
        Launch::Open => hand_over(&folder, &request, &Caller::Command)?,
    };
    match answer {
        Answer::GuidedReady { .. } => println!("{answer}"),
        Answer::Opening { .. }
        | Answer::Writing { .. }
        | Answer::Reloaded { .. }
        | Answer::KeptInBackground { .. } => {
            println!("{answer}");
            if ToursOnDisk::of(folders, &ReviewSubject::from(pr.clone())).opens_basic() {
                println!("{pr} opened in Basic mode");
            }
        }
        other => anyhow::bail!("{other}"),
    }
    Ok(())
}

fn settings() -> anyhow::Result<()> {
    let answer = hand_over(
        &AppFolder::of(&AppFolders::running()?),
        &Request::ShowSettings,
        &Caller::Command,
    )?;
    match answer {
        Answer::ShowingSettings => {
            println!("{answer}");
            Ok(())
        }
        other => anyhow::bail!("{other}"),
    }
}

/// A launch from the OS launcher first runs itself again through the login shell, then shows the PR
/// switcher; `guided-review` with no command shows it too.
fn launch() -> anyhow::Result<()> {
    let folders = AppFolders::running()?;
    let install = ReleaseInstall::in_home(folders.home());
    let desktop_entry = install.desktop_entry();
    let launched_app = std::env::var_os(desktop_entry.launched_app_variable());
    let shell = std::env::var_os("SHELL");
    let is_in_login_shell = std::env::var_os(IN_LOGIN_SHELL).is_some();
    if let Some(relaunch) = desktop_entry.relaunch_in_login_shell(
        launched_app.as_deref(),
        shell.as_deref(),
        is_in_login_shell,
    ) {
        let error = std::process::Command::new(&relaunch.program)
            .args(&relaunch.args)
            .env(IN_LOGIN_SHELL, "1")
            .exec();
        return Err(error).with_context(|| {
            format!(
                "could not run the app through {}",
                relaunch.program.display()
            )
        });
    }
    let caller = if desktop_entry.is_launch_of_the_app(launched_app.as_deref()) {
        Caller::Launcher
    } else {
        Caller::Command
    };
    let answer = hand_over(&AppFolder::of(&folders), &Request::ShowSwitcher, &caller)?;
    match answer {
        Answer::ShowingSwitcher => {
            println!("{answer}");
            Ok(())
        }
        other => anyhow::bail!("{other}"),
    }
}

fn install_app() -> anyhow::Result<()> {
    let exe = guided_review::platform::running_binary()
        .context("could not find the guided-review binary")?;
    let install = ReleaseInstall::in_home(AppFolders::running()?.home());
    let placed = install.bring_desktop_entry_up_to_date(&exe)?;
    let (name, folder) = (
        install.desktop_entry().name(),
        install.desktop_entry().folder(),
    );
    match placed {
        EntryPlaced::Refreshed => println!("{name} is in {folder}"),
        EntryPlaced::Current => println!("{name} in {folder} is up to date"),
        EntryPlaced::NotInRelease => {
            anyhow::bail!("only a release install carries {name}; this guided-review is not one")
        }
    }
    Ok(())
}

fn serve() -> anyhow::Result<()> {
    let folders = AppFolders::running()?;
    match AppListener::bind(&AppFolder::of(&folders)) {
        Ok(listener) => {
            guided_review::app::serve(listener, folders);
            Ok(())
        }
        Err(BindError::AlreadyRunning) => Ok(()),
        Err(BindError::Failed(error)) => Err(error),
    }
}
