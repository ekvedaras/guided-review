use anyhow::Context;
use clap::{Parser, Subcommand};

use guided_review::agent_reply::{AgentReply, ReplyTo, agent_reply, why_reply};
use guided_review::app_folders::AppFolders;
use guided_review::app_link::{
    Answer, AppFolder, AppListener, BindError, Caller, OpenRequest, Request, hand_over,
    restart as restart_app,
};
use guided_review::claude_cli::{ClaudeEvalWriter, ClaudeTourJudge};
use guided_review::config::Config;
use guided_review::eval::{Eval, EvalStore, Skill, TourSource};
use guided_review::findings::FindingId;
use guided_review::findings::FindingsFile;
use guided_review::findings_check;
use guided_review::findings_done::findings_done;
use guided_review::fix_reply::fix_reply;
use guided_review::git_checkout::GitCheckouts;
use guided_review::github::GitHub;
use guided_review::instructions::SummaryInstructions;
use guided_review::opened_subject::{OpenedChangeSet, OpenedSubject};
use guided_review::outline::Outline;
use guided_review::pieces;
use guided_review::platform::{EntryPlaced, IN_LOGIN_SHELL, write_desktop_entry_templates};
use guided_review::pull_request::PullRequestUrl;
use guided_review::review_brief::ReviewBrief;
use guided_review::review_subject::ReviewSubject;
use guided_review::show::Show;
use guided_review::test_pairing::SubjectCandidates;
use guided_review::timestamp::UtcTimestamp;
use guided_review::tour::{GuidedTour, StepId, Tour, ToursOnDisk};
use guided_review::tour_check;
use guided_review::update::{AppVersion, ReleaseInstall};
use guided_review::version::Version;
use guided_review::viewer::CheckoutPort;
use guided_review::viewer::FixReply;
use guided_review::viewer::{RefreshBrief, ReviewStateFile};
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
    /// Print what the review agent is told: the protocol, the review instructions, what to skip
    ReviewBrief { subject: ReviewSubject },
    /// Print where the findings file of a review lives
    FindingsPath { subject: ReviewSubject },
    /// Pin the findings file of a review to its commits, then list what is wrong with its anchors
    CheckFindings { subject: ReviewSubject },
    /// Tell the app the review agent's findings file is written and checked
    FindingsDone { subject: ReviewSubject },
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
    /// Open the app on the tour of a PR, or of the branch of a clone's folder and its uncommitted files
    Open {
        /// A PR link, a `local:` key, or a folder inside a clone
        target: OpenTarget,
    },
    /// Quit the running app of this build and open a PR or a folder's branch in a new one
    Restart {
        /// A PR link, a `local:` key, or a folder inside a clone
        target: OpenTarget,
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
    /// Answer a reviewer's hand-off on its GitHub thread, the reply starting with the robot mark, or a
    /// question on a finding on its thread
    AgentReply {
        subject: ReviewSubject,
        /// The hand-off id from the message, like `h0`, or the finding's id
        id: String,
        /// The answer in GitHub markdown; `-` reads it from stdin, which keeps its line breaks
        #[arg(long, required_unless_present = "working", conflicts_with = "working")]
        body: Option<String>,
        /// Post "Working on it…" now; the answer with --body replaces it
        #[arg(long)]
        working: bool,
    },
    /// Say where the tour agent is with one finding of a fix request
    FixReply {
        subject: ReviewSubject,
        /// The finding id from the message
        finding: String,
        /// You started on this finding
        #[arg(long, group = "outcome")]
        working: bool,
        /// You fixed it, in this commit
        #[arg(long, group = "outcome", value_name = "COMMIT")]
        fixed: Option<String>,
        /// You did not fix it; give the reason with --body
        #[arg(long, group = "outcome", requires = "body")]
        not_fixed: bool,
        /// You need a decision from the reviewer before you fix it; give the question with --body
        #[arg(long, group = "outcome", requires = "body")]
        question: bool,
        /// What you did, the reason or the question, in markdown; `-` reads it from stdin, which keeps its line breaks
        #[arg(long, requires = "outcome", conflicts_with = "working")]
        body: Option<String>,
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
        Command::Outline { subject } => outline(&subject),
        Command::Show {
            subject,
            ids,
            around,
        } => show(&subject, &ids, around),
        Command::TourPath { subject } => tour_path(&subject),
        Command::ReviewBrief { subject } => review_brief(&subject),
        Command::FindingsPath { subject } => findings_path(&subject),
        Command::CheckFindings { subject } => check_findings(&subject),
        Command::FindingsDone { subject } => {
            println!("{}", findings_done(&AppFolders::running()?, &subject)?);
            Ok(())
        }
        Command::Check { subject } => check(&subject),
        Command::Repin { subject } => repin_command(&subject),
        Command::Refresh { subject, done } => refresh(&subject, done),
        Command::Open { target } => open(&target, &Launch::Open),
        Command::Restart {
            target,
            at,
            replacing,
        } => open(&target, &Launch::Restart { at, replacing }),
        Command::Settings => settings(),
        Command::InstallApp => install_app(),
        Command::DesktopEntryTemplate { out } => {
            write_desktop_entry_templates(&out, Version::running())
        }
        Command::AgentReply {
            subject,
            id,
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
            let folders = AppFolders::running()?;
            let said = match ReplyTo::of(&folders, &subject, &id)? {
                ReplyTo::Finding(finding) => why_reply(&folders, &subject, &finding, &reply)?,
                ReplyTo::HandOff(hand_off) => {
                    agent_reply(&GitHub::from_env(), &folders, &subject, hand_off, &reply)?
                }
            };
            println!("{said}");
            Ok(())
        }
        Command::FixReply {
            subject,
            finding,
            working: _,
            fixed,
            not_fixed: _,
            question,
            body,
        } => {
            let reply = match (fixed, body) {
                (Some(commit), body) => FixReply::Fixed {
                    commit,
                    summary: body.map(read_body).transpose()?,
                },
                (None, Some(body)) => {
                    let text = read_body(body)?;
                    if question {
                        FixReply::Question { text }
                    } else {
                        FixReply::NotFixed { reason: text }
                    }
                }
                (None, None) => FixReply::Working,
            };
            let finding = FindingId::try_from(finding).map_err(anyhow::Error::msg)?;
            println!(
                "{}",
                fix_reply(&AppFolders::running()?, &subject, &finding, &reply)?
            );
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

/// What `open` and `restart` take: a PR link, a local change set's key, or a folder, which stands for
/// the branch it is on.
#[derive(Clone)]
enum OpenTarget {
    Subject(ReviewSubject),
    Folder(std::path::PathBuf),
}

impl std::str::FromStr for OpenTarget {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> anyhow::Result<Self> {
        if std::path::Path::new(text).exists() {
            return Ok(Self::Folder(text.into()));
        }
        text.parse().map(Self::Subject).map_err(|error: anyhow::Error| {
            anyhow::anyhow!(
                "{text} is neither a GitHub PR link, a local change set key nor a folder ({error:#})"
            )
        })
    }
}

/// A `--body` value: `-` reads stdin, which keeps its line breaks.
fn read_body(body: String) -> anyhow::Result<String> {
    let body = match body.as_str() {
        "-" => std::io::read_to_string(std::io::stdin())?,
        _ => body,
    };
    Ok(body.trim_end().to_owned())
}

fn tour_path(subject: &ReviewSubject) -> anyhow::Result<()> {
    println!(
        "{}",
        Tour::path_for(&AppFolders::running()?, subject).display()
    );
    Ok(())
}

fn opened(subject: &ReviewSubject) -> anyhow::Result<OpenedSubject> {
    Ok(OpenedSubject::in_folders(
        subject,
        &AppFolders::running()?,
        &std::env::current_dir().context("could not read the current folder")?,
    ))
}

/// A config that cannot be read must not stop `outline` or `check`, which work without it.
fn summary_instructions() -> anyhow::Result<SummaryInstructions> {
    let folders = AppFolders::running()?;
    match Config::load_from(&folders) {
        Ok(config) => Ok(config.instructions(&folders)),
        Err(error) => {
            eprintln!("warning: summary instructions not read: {error:#}");
            Ok(SummaryInstructions::none())
        }
    }
}

fn outline(subject: &ReviewSubject) -> anyhow::Result<()> {
    let summary_instructions = summary_instructions()?;
    let code = opened(subject)?.code_to_read()?;
    let (base, files) = code.pieces_since_merge_base()?;
    print!(
        "{}",
        Outline {
            subject,
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

fn show(subject: &ReviewSubject, ids: &[String], around: bool) -> anyhow::Result<()> {
    let code = opened(subject)?.code_to_read()?;
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

fn review_brief(subject: &ReviewSubject) -> anyhow::Result<()> {
    let brief = ReviewBrief::of(&AppFolders::running()?, subject);
    if let Some(warning) = brief.warning() {
        eprintln!("warning: {warning}");
    }
    print!("{brief}");
    Ok(())
}

fn findings_path(subject: &ReviewSubject) -> anyhow::Result<()> {
    println!(
        "{}",
        Tour::findings_path_for(&AppFolders::running()?, subject).display()
    );
    Ok(())
}

fn check_findings(subject: &ReviewSubject) -> anyhow::Result<()> {
    let opened = opened(subject)?;
    let subject = &opened.subject();
    let folders = AppFolders::running()?;
    let path = Tour::findings_path_for(&folders, subject);
    let mut findings = FindingsFile::load_of(&path, subject)?;

    let code = opened.code_to_read()?;
    if findings.pin.head.is_empty() {
        // The agent's checkout, in its own folder or the worktree it made, stands at the reviewed head,
        // which a PR may have moved past since.
        let agent_head = std::env::current_dir()
            .ok()
            .and_then(|folder| GitCheckouts.head_of(&folder).ok());
        findings.pin.pin_to(
            subject,
            &code.merge_base()?,
            agent_head.as_ref().unwrap_or(&code.tips.head),
        );
    } else if findings.pin.head != code.tips.head {
        println!("warning: {}", subject.moved_on());
    }
    findings.save(&path)?;
    let files = pieces::cut(code.source.as_ref(), &findings.pin.base, &findings.pin.head)?;

    let report = findings_check::check(&findings, &files);
    print!("{report}");
    if !report.errors.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

fn check(subject: &ReviewSubject) -> anyhow::Result<()> {
    let opened = opened(subject)?;
    // The tour pin keeps a local change set's repo, so a PR of the repo finds the review without git.
    let subject = &opened.subject();
    let tour_path = Tour::path_for(&AppFolders::running()?, subject);
    let mut tour = GuidedTour::load_of(&tour_path, subject)?;

    let code = opened.code_to_read()?;
    if tour.pin.head.is_empty() {
        tour.pin
            .pin_to(subject, &code.merge_base()?, &code.tips.head);
    } else {
        if tour.pin.pr.is_empty() && tour.pin.local.is_none() {
            tour.pin.name_subject(subject);
        }
        if tour.pin.head != code.tips.head {
            println!("warning: {}; run `guided-review repin`", subject.moved_on());
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

fn repin_command(subject: &ReviewSubject) -> anyhow::Result<()> {
    print!("{}", opened(subject)?.repin()?);
    Ok(())
}

fn refresh(subject: &ReviewSubject, done: bool) -> anyhow::Result<()> {
    let folders = AppFolders::running()?;
    let state_file = ReviewStateFile::for_subject(&folders, subject);
    if done {
        state_file.update(guided_review::viewer::ReviewState::finish_refresh)?;
        println!("refresh done: marks cleared");
        return Ok(());
    }
    let tour = GuidedTour::load_of(&Tour::path_for(&folders, subject), subject)?;
    let code = opened(subject)?.code_at(&tour.pin)?;
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

fn open(target: &OpenTarget, launch: &Launch) -> anyhow::Result<()> {
    let folders = &AppFolders::running()?;
    let subject = match target {
        OpenTarget::Subject(subject) => subject.clone(),
        OpenTarget::Folder(path) => ReviewSubject::from(OpenedChangeSet::change_set_at(path)?),
    };
    let tours = ToursOnDisk::of(folders, &subject);
    if let Launch::Restart { at: Some(step), .. } = launch
        && tours.opens_basic()
    {
        anyhow::bail!(
            "{subject} opens in Basic mode, which has no steps, so there is no step `{step}` to restart at"
        );
    }
    if tours == ToursOnDisk::None {
        opened(&subject)?.make_basic_tour_if_no_tour()?;
    }
    let folder = AppFolder::of(folders);
    let request = Request::Open(OpenRequest {
        subject: subject.to_string(),
        folder: std::env::current_dir().context("could not read the current folder")?,
        terminal: std::env::var("HC_TERMINAL_ID").ok(),
        waits_for_build: true,
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
    let told = answer.clone().about_open(true)?;
    println!("{told}");
    if !matches!(answer, Answer::GuidedReady { .. })
        && ToursOnDisk::of(folders, &subject).opens_basic()
    {
        println!("{subject} opened in Basic mode");
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
