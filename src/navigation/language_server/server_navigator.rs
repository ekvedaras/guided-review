//! The adapter one review asks: a name in a head line of a language with a server is asked of the
//! codebase's server in the server checkout, and the by-name lookup answers whatever the server
//! cannot.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::code_source::CodeSource;
use crate::codebase::Codebase;
use crate::language::Language;

use super::answers::Places;
use super::pool::Pool;
use super::positions::uri_of;
use super::rpc::Failure;
use super::{
    Answer, AnsweredBy, Checkout, CodeNavigator, FallbackReason, NavigationRequest, ServerLaunch,
    ServerName, Waiting,
};

/// How often a question the server dropped because files changed is asked again.
const MODIFIED_RETRIES: u32 = 5;
const MODIFIED_PAUSE: Duration = Duration::from_millis(250);

pub(super) struct ServerNavigator {
    pub(super) pool: Arc<Pool>,
    pub(super) codebase: Codebase,
    pub(super) by_name: Arc<dyn CodeNavigator>,
}

impl CodeNavigator for ServerNavigator {
    fn navigate(
        &self,
        code: &dyn CodeSource,
        request: &NavigationRequest,
        waiting: &dyn Fn(Waiting),
    ) -> anyhow::Result<Answer> {
        let by_name = |because: Option<FallbackReason>| -> anyhow::Result<Answer> {
            let mut answer = self.by_name.navigate(code, request, waiting)?;
            answer.answered_by = AnsweredBy::ByName { because };
            Ok(answer)
        };
        let Some(language) = Language::for_path(request.at.path.as_str()) else {
            return by_name(None);
        };
        let Some(clone) = code.local_clone() else {
            return by_name(Some(FallbackReason::new(&format!(
                "{} needs a local clone, and the code is read from GitHub",
                ServerName::of(language)
            ))));
        };
        let launch = match self.pool.launch_for(language, &self.codebase, waiting) {
            Ok(launch) => launch,
            Err(reason) => return by_name(Some(reason)),
        };
        let answered = self
            .pool
            .at_head(&self.codebase, clone, &request.sha, |checkout| {
                self.answer(code, checkout, &launch, language, request, waiting)
            });
        match answered {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(reason)) => by_name(Some(FallbackReason::new(&reason))),
            Err(error) => by_name(Some(FallbackReason::new(&format!(
                "{} could not get the reviewed head checked out: {error:#}",
                launch.name
            )))),
        }
    }
}

impl ServerNavigator {
    fn answer(
        &self,
        code: &dyn CodeSource,
        checkout: &Checkout,
        launch: &ServerLaunch,
        language: Language,
        request: &NavigationRequest,
        waiting: &dyn Fn(Waiting),
    ) -> Result<Answer, String> {
        let name = &launch.name;
        let server = self
            .pool
            .server(&self.codebase, launch, checkout, waiting)?;
        let failed = |failure: Failure| {
            if failure == Failure::Died {
                server.forget();
            }
            failure.of_question(name)
        };
        // A server that sat on a question may sit on every later one, so a later hover gets a new one.
        let unanswered = |failure: Failure| {
            if failure == Failure::Silent {
                server.forget_and_stop();
            }
            failed(failure)
        };
        server.wait_until_quiet(waiting).map_err(failed)?;
        let folder = checkout.folder.as_path();
        let file = request.at.path.under(folder);
        let text = std::fs::read_to_string(&file)
            .map_err(|error| format!("{} could not read {}: {error}", name, request.at.path))?;
        server.open(&file, &text, language).map_err(failed)?;
        let character = server
            .character(&text, request.at.line, request.column)
            .ok_or_else(|| format!("{name} was asked about a place the line does not have"))?;
        let line = u32::try_from(usize::from(request.at.line).saturating_sub(1))
            .map_err(|error| format!("the line number is too large: {error}"))?;
        let place = json!({
            "textDocument": {"uri": uri_of(&file)},
            "position": {"line": line, "character": character},
        });
        let references = json!({
            "textDocument": {"uri": uri_of(&file)},
            "position": {"line": line, "character": character},
            "context": {"includeDeclaration": false},
        });
        let ask_all = || -> Result<[Value; 3], Failure> {
            Ok([
                server.ask("textDocument/definition", &place)?,
                server.ask("textDocument/references", &references)?,
                server.ask("textDocument/hover", &place)?,
            ])
        };
        let mut retries = 0;
        let [definitions, usages, hover] = loop {
            match ask_all() {
                Err(Failure::Modified) if retries < MODIFIED_RETRIES => {
                    retries += 1;
                    // The server drops questions while it takes in changed files, and it says it is busy
                    // only a moment after.
                    std::thread::sleep(MODIFIED_PAUSE);
                    server.wait_until_quiet(waiting).map_err(failed)?;
                }
                asked => break asked.map_err(unanswered)?,
            }
        };
        server.answered();
        let places = Places {
            code,
            checkout: folder,
            sha: &request.sha,
        };
        Ok(Answer {
            lookup: places.lookup(&request.word, &definitions, &usages, &hover),
            answered_by: AnsweredBy::LanguageServer(name.clone()),
        })
    }
}
