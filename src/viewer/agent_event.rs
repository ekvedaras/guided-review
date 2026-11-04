use serde_json::Value;

use crate::json::at;

/// What the AI did, as the viewer cares about it. Nothing above this file sees JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    TextDelta(String),
    ToolUse { summary: String },
    Done { session_id: String },
    Error(String),
}

impl AgentEvent {
    /// Reads one `stream-json` line of the `claude` CLI. Lines the viewer has no use for give no events.
    pub fn from_line(line: &str) -> Vec<Self> {
        let Ok(json) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        match at(&json, "/type").as_str() {
            Some("stream_event") => text_delta(&json).into_iter().collect(),
            Some("assistant") => tool_uses(&json),
            Some("result") => vec![outcome(&json)],
            _ => Vec::new(),
        }
    }
}

fn text_delta(json: &Value) -> Option<AgentEvent> {
    let delta = at(json, "/event/delta");
    (at(delta, "/type") == "text_delta")
        .then(|| AgentEvent::TextDelta(at(delta, "/text").as_str().unwrap_or_default().to_owned()))
}

fn tool_uses(json: &Value) -> Vec<AgentEvent> {
    at(json, "/message/content")
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| at(block, "/type") == "tool_use")
        .map(|block| AgentEvent::ToolUse {
            summary: describe_tool_use(block),
        })
        .collect()
}

fn describe_tool_use(block: &Value) -> String {
    let name = at(block, "/name").as_str().unwrap_or("tool");
    let input = at(block, "/input");
    let target = ["file_path", "path", "pattern", "command"]
        .iter()
        .find_map(|key| input.get(key).and_then(Value::as_str));
    match target {
        Some(target) => format!("{name} {target}"),
        None => name.to_owned(),
    }
}

fn outcome(json: &Value) -> AgentEvent {
    if at(json, "/is_error") == true {
        AgentEvent::Error(
            at(json, "/result")
                .as_str()
                .unwrap_or("the AI failed")
                .to_owned(),
        )
    } else {
        AgentEvent::Done {
            session_id: at(json, "/session_id")
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        }
    }
}
