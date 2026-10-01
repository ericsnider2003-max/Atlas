//! `atlas mcp` (`mcpserve`, 1 Oct 2026): Atlas as an MCP server, a doorway
//! to the running Atlas's hub.

use atlas::mcpserve::{answer_message, serve, Hub};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::time::Duration;

/// A hub that answers the Talk page like the daemon does: a post is queued,
/// the next look finds it answered.
#[derive(Default)]
struct FakeHub {
    posted: RefCell<Vec<(String, String)>>,
    looks: RefCell<u32>,
    down: bool,
}

impl Hub for FakeHub {
    fn get(&self, path: &str) -> Result<String, String> {
        if self.down {
            return Err("Atlas isn't answering on this machine -- is it running?".into());
        }
        match path {
            "/hub/live.json" => Ok(json!({ "status": "idle", "waiting": 2 }).to_string()),
            "/hub/talk.json" => {
                *self.looks.borrow_mut() += 1;
                let posted = self.posted.borrow();
                let said = posted.last().map(|(_, b)| atlas::hub::form_field(b, "text").unwrap_or_default());
                Ok(match (said, *self.looks.borrow()) {
                    (Some(s), 1) => json!({ "recent": [], "pending": [s], "thinking": false }),
                    (Some(s), _) => json!({ "recent": [{ "said": s, "reply": "It's ten past four." }], "pending": [], "thinking": false }),
                    (None, _) => json!({ "recent": [], "pending": [], "thinking": false }),
                }
                .to_string())
            }
            _ => Err(format!("Atlas said 404 to {path}")),
        }
    }
    fn post_form(&self, path: &str, body: &str) -> Result<(), String> {
        self.posted.borrow_mut().push((path.into(), body.into()));
        Ok(())
    }
}

fn rpc(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

#[test]
fn it_introduces_itself_and_lists_three_tools_none_of_which_approve() {
    let hub = FakeHub::default();
    let init = answer_message(&rpc(1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } })), &hub, Duration::ZERO).unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "atlas");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    // A notification gets no answer.
    assert!(answer_message(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), &hub, Duration::ZERO).is_none());
    let list = answer_message(&rpc(2, "tools/list", json!({})), &hub, Duration::ZERO).unwrap();
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["atlas_now", "atlas_recent", "atlas_ask"]);
    assert!(!names.iter().any(|n| n.contains("approve") || n.contains("deny")));
}

#[test]
fn asking_goes_through_the_talk_page_and_waits_for_the_reply() {
    let hub = FakeHub::default();
    let r = answer_message(&rpc(3, "tools/call", json!({ "name": "atlas_ask", "arguments": { "text": "what time is it?" } })), &hub, Duration::from_secs(5)).unwrap();
    assert_eq!(r["result"]["content"][0]["text"], "It's ten past four.");
    assert_eq!(r["result"]["isError"], false);
    let posted = hub.posted.borrow();
    assert_eq!(posted[0].0, "/hub/talk");
    assert_eq!(atlas::hub::form_field(&posted[0].1, "text").as_deref(), Some("what time is it?"));
}

#[test]
fn a_closed_atlas_is_said_plainly_as_an_error() {
    let hub = FakeHub { down: true, ..Default::default() };
    let r = answer_message(&rpc(4, "tools/call", json!({ "name": "atlas_now", "arguments": {} })), &hub, Duration::ZERO).unwrap();
    assert_eq!(r["result"]["isError"], true);
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("is it running"));
    let r = answer_message(&rpc(5, "nonsense", json!({})), &hub, Duration::ZERO).unwrap();
    assert_eq!(r["error"]["code"], -32601);
}

#[test]
fn over_stdio_one_line_in_is_one_line_out() {
    let hub = FakeHub::default();
    let input = format!(
        "{}\n{}\n\n{}\n",
        rpc(1, "initialize", json!({})),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        rpc(2, "tools/call", json!({ "name": "atlas_now", "arguments": {} }))
    );
    let mut out = Vec::new();
    serve(input.as_bytes(), &mut out, &hub);
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["id"], 2);
    assert!(lines[1]["result"]["content"][0]["text"].as_str().unwrap().contains("\"waiting\":2"));
}

#[test]
fn the_talk_page_is_readable_as_data_with_the_token() {
    let req = atlas::server::parse_request("GET /hub/talk.json HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer abc\r\n", "").unwrap();
    assert!(matches!(atlas::server::route(&req), Some(atlas::server::Action::TalkJson)));
}
