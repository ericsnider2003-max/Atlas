//! Atlas as a Model Context Protocol **server** (1 Oct 2026, research report
//! item 29): `atlas mcp` lets Eric's other AI tools -- Claude Desktop,
//! Claude Code, anything that speaks MCP -- see what Atlas is doing and ask
//! it things.
//!
//! **It is a doorway to the Atlas already running, not a second Atlas.** A
//! second daemon was the "two versions of you" bug of 30 Sep. So this
//! process holds no state of its own: every tool is a request to the
//! running Atlas's hub on loopback, with the install's hub token, exactly
//! what the phone app does.
//!
//! **What it can do, and what it can't.**
//! * `atlas_now` -- what Atlas is working on, what's ready, what's waiting
//!   for Eric (`/hub/live.json`). Read only.
//! * `atlas_recent` -- the last exchanges on the Talk page
//!   (`/hub/talk.json`). Read only.
//! * `atlas_ask` -- say something to Atlas as if typed on the Talk page
//!   (`/hub/talk`), and wait for its reply. It goes through the same front
//!   door, the same policy and the same approvals as anything typed: when
//!   Atlas asks "go ahead?", **this connection cannot answer** -- there is
//!   no approve tool, on purpose. Eric approves on the hub, by voice, or on
//!   the phone.
//!
//! The protocol is the stdio transport of MCP revision 2025-06-18
//! (newline-delimited JSON-RPC 2.0): `initialize`, `notifications/
//! initialized`, `ping`, `tools/list`, `tools/call`. The message shapes are
//! the ones `mcp.rs`, Atlas's client, already speaks.

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

/// How the server reaches the running Atlas. A trait so the protocol is
/// tested without a daemon.
pub trait Hub {
    /// A GET on the hub: the body, or why not.
    fn get(&self, path: &str) -> Result<String, String>;
    /// A form POST on the hub.
    fn post_form(&self, path: &str, body: &str) -> Result<(), String>;
}

/// The running Atlas on this machine, over loopback with its hub token.
pub struct LocalHub {
    pub port: u16,
    pub token: String,
}

impl Hub for LocalHub {
    fn get(&self, path: &str) -> Result<String, String> {
        let r = crate::http::get_with_token(&format!("127.0.0.1:{}", self.port), path, &self.token, Duration::from_secs(10))
            .map_err(|e| format!("Atlas isn't answering on this machine ({e}) -- is it running?"))?;
        if r.ok() { Ok(r.body) } else { Err(format!("Atlas said {} to {path}", r.status)) }
    }
    fn post_form(&self, path: &str, body: &str) -> Result<(), String> {
        let r = crate::http::post_json_with_token(&format!("127.0.0.1:{}", self.port), path, body, &self.token, Duration::from_secs(10))
            .map_err(|e| format!("Atlas isn't answering on this machine ({e}) -- is it running?"))?;
        // The hub answers a form with a redirect back to its page.
        if r.ok() || (300..400).contains(&r.status) { Ok(()) } else { Err(format!("Atlas said {} to {path}", r.status)) }
    }
}

/// The revision this server speaks.
pub const PROTOCOL_VERSION: &str = crate::mcp::PROTOCOL_VERSION;

/// How long `atlas_ask` waits for a reply before saying it's still working.
pub const ASK_WAIT: Duration = Duration::from_secs(120);

/// The tools, as `tools/list` gives them.
fn tool_list() -> Value {
    json!([
        {
            "name": "atlas_now",
            "description": "What Eric's assistant Atlas is doing right now: its status, the job it's working on, what's ready, and how many things are waiting for Eric.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "atlas_recent",
            "description": "The last few exchanges between Eric and Atlas on Atlas's Talk page: what was said and what Atlas replied.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "atlas_ask",
            "description": "Say something to Atlas, as if Eric typed it on the Talk page, and get Atlas's reply. Questions are answered; tasks are started under Atlas's own rules. Anything Atlas needs approval for waits for Eric -- this tool cannot approve.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string", "description": "What to say to Atlas, in plain words." } },
                "required": ["text"]
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "openWorldHint": false }
        }
    ])
}

fn text_result(text: &str, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

/// The last exchange whose words were `said`, from `/hub/talk.json`, once
/// Atlas has finished with it: `None` while it's queued or being answered.
fn reply_to(talk: &Value, said: &str) -> Option<String> {
    let pending = talk["pending"].as_array().is_some_and(|p| p.iter().any(|s| s.as_str() == Some(said)));
    if pending || talk["thinking"].as_bool() == Some(true) {
        return None;
    }
    let last = talk["recent"].as_array()?.last()?;
    (last["said"].as_str() == Some(said)).then(|| last["reply"].as_str().unwrap_or_default().to_string())
}

/// One tool call.
fn call(hub: &dyn Hub, name: &str, args: &Value, wait: Duration) -> Value {
    match name {
        "atlas_now" => match hub.get("/hub/live.json") {
            Ok(body) => text_result(&body, false),
            Err(e) => text_result(&e, true),
        },
        "atlas_recent" => match hub.get("/hub/talk.json") {
            Ok(body) => text_result(&body, false),
            Err(e) => text_result(&e, true),
        },
        "atlas_ask" => {
            let text = args["text"].as_str().unwrap_or("").trim().to_string();
            if text.is_empty() {
                return text_result("Say what to ask Atlas, in `text`.", true);
            }
            if let Err(e) = hub.post_form("/hub/talk", &format!("text={}", crate::research::urlencode(&text))) {
                return text_result(&e, true);
            }
            let until = Instant::now() + wait;
            loop {
                if let Ok(body) = hub.get("/hub/talk.json") {
                    if let Some(reply) = serde_json::from_str::<Value>(&body).ok().and_then(|v| reply_to(&v, &text)) {
                        return text_result(&reply, false);
                    }
                }
                if Instant::now() >= until {
                    return text_result(
                        "Atlas has it and is still working on it. Its reply will be on the Talk page; ask atlas_recent later.",
                        false,
                    );
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        _ => text_result(&format!("There's no tool called {name}."), true),
    }
}

/// Answer one JSON-RPC message. `None` for a notification, which gets no
/// answer.
pub fn answer_message(msg: &Value, hub: &dyn Hub, wait: Duration) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "atlas", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Atlas is Eric's personal assistant running on his computer. Use atlas_now and atlas_recent to see what it's doing; atlas_ask to ask it something or hand it a task. It can't approve anything on Eric's behalf."
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tool_list() }),
        "tools/call" => call(hub, msg["params"]["name"].as_str().unwrap_or(""), &msg["params"]["arguments"], wait),
        _ => {
            return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("no method {method}") } }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// Serve on `input`/`output` until the client closes `input`.
pub fn serve(input: impl BufRead, mut output: impl Write, hub: &dyn Hub) {
    for line in input.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => answer_message(&msg, hub, ASK_WAIT),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("not JSON: {e}") } })),
        };
        if let Some(r) = reply {
            if writeln!(output, "{r}").and_then(|_| output.flush()).is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_is_taken_only_once_atlas_has_finished() {
        let talk = json!({ "recent": [{ "said": "what time is it", "reply": "Ten past four." }], "pending": [], "thinking": false });
        assert_eq!(reply_to(&talk, "what time is it").as_deref(), Some("Ten past four."));
        let busy = json!({ "recent": [{ "said": "what time is it", "reply": "" }], "pending": [], "thinking": true });
        assert_eq!(reply_to(&busy, "what time is it"), None);
        let queued = json!({ "recent": [], "pending": ["what time is it"], "thinking": false });
        assert_eq!(reply_to(&queued, "what time is it"), None);
    }
}
