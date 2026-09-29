//! Chrome DevTools Protocol backend.
//!
//! This is the answer to "navigate behind the scenes": a real browser Atlas
//! drives over a local websocket. It clicks by CSS selector rather than by
//! pixel, reads the DOM directly instead of screenshotting and guessing, and
//! runs against a headless instance so your own Chrome window is untouched.
//!
//! Deliberate choice: almost everything goes through `Runtime.evaluate` rather
//! than the Input and DOM domains. Telling the page `document.querySelector(x)
//! .click()` is one round trip and hits the right element; synthesising a
//! mouse event at computed coordinates is three round trips and misses when
//! the page scrolls between them.

use crate::error::{AtlasError, Result};
use crate::ws::WebSocket;
use serde_json::{json, Value};
use std::time::Duration;

pub struct Cdp {
    ws: WebSocket,
    next_id: u64,
    timeout: Duration,
}

impl Cdp {
    /// Connect to a Chrome started with `--remote-debugging-port`.
    pub fn connect(ws_url: &str, timeout: Duration) -> Result<Cdp> {
        Ok(Cdp { ws: WebSocket::connect(ws_url, timeout)?, next_id: 0, timeout })
    }

    /// Raw protocol call.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let msg = json!({ "id": id, "method": method, "params": params });
        self.ws.send_text(&msg.to_string())?;

        // Chrome interleaves events with responses; match on id and drop the
        // rest rather than assuming the next message is ours.
        let deadline = std::time::Instant::now() + self.timeout * 4;
        loop {
            if std::time::Instant::now() > deadline {
                return Err(AtlasError::Platform(format!("{method} timed out")));
            }
            let raw = self.ws.recv_text()?;
            let v: Value = serde_json::from_str(&raw)
                .map_err(|e| AtlasError::Platform(format!("bad CDP json: {e}")))?;
            if v.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(err) = v.get("error") {
                return Err(AtlasError::Platform(format!("{method}: {err}")));
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// Run JavaScript in the page and return its value.
    pub fn eval(&mut self, js: &str) -> Result<Value> {
        let r = self.call(
            "Runtime.evaluate",
            json!({
                "expression": js,
                "returnByValue": true,
                "awaitPromise": true,
                "userGesture": true
            }),
        )?;
        if let Some(ex) = r.get("exceptionDetails") {
            let text = ex.get("text").and_then(Value::as_str).unwrap_or("script error");
            return Err(AtlasError::Platform(format!("page threw: {text}")));
        }
        Ok(r.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    pub fn navigate(&mut self, url: &str) -> Result<()> {
        self.call("Page.enable", json!({}))?;
        self.call("Page.navigate", json!({ "url": url }))?;
        Ok(())
    }

    /// Readable page text, with script and style stripped by the browser
    /// itself — far more accurate than parsing HTML after the fact.
    pub fn text(&mut self) -> Result<String> {
        let v = self.eval(
            "(() => { const c = document.body ? document.body.cloneNode(true) : null;
              if (!c) return '';
              c.querySelectorAll('script,style,noscript,svg').forEach(e => e.remove());
              return (c.innerText || '').replace(/\\s+/g, ' ').trim(); })()",
        )?;
        Ok(v.as_str().unwrap_or_default().to_string())
    }

    pub fn title(&mut self) -> Result<String> {
        Ok(self.eval("document.title")?.as_str().unwrap_or_default().to_string())
    }

    pub fn url(&mut self) -> Result<String> {
        Ok(self.eval("location.href")?.as_str().unwrap_or_default().to_string())
    }

    /// Click by selector. Returns an error naming the selector if it is not
    /// on the page, rather than silently doing nothing.
    pub fn click(&mut self, selector: &str) -> Result<()> {
        let ok = self.eval(&click_js(selector))?;
        if ok.as_bool() == Some(true) {
            Ok(())
        } else {
            Err(AtlasError::Platform(format!("nothing matches '{selector}'")))
        }
    }

    pub fn fill(&mut self, selector: &str, text: &str) -> Result<()> {
        let ok = self.eval(&fill_js(selector, text))?;
        if ok.as_bool() == Some(true) {
            Ok(())
        } else {
            Err(AtlasError::Platform(format!("no input matches '{selector}'")))
        }
    }

    pub fn scroll(&mut self, dy: i32) -> Result<()> {
        self.eval(&format!("window.scrollBy(0, {dy}); true"))?;
        Ok(())
    }

    /// Poll for an element. Pages load asynchronously; assuming the DOM is
    /// ready right after navigate is the most common way browser automation
    /// breaks.
    pub fn wait_for(&mut self, selector: &str, timeout_ms: u64) -> Result<bool> {
        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            if self.eval(&exists_js(selector))?.as_bool() == Some(true) {
                return Ok(true);
            }
            if std::time::Instant::now() > deadline {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(120));
        }
    }

    pub fn links(&mut self) -> Result<Vec<String>> {
        let v = self.eval(
            "Array.from(document.querySelectorAll('a[href]'))
             .map(a => a.href).filter(h => h.startsWith('http')).slice(0, 200)",
        )?;
        Ok(v.as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default())
    }

    pub fn close(&mut self) {
        self.ws.close();
    }
}

// --- JS builders, kept separate so they can be tested without a browser ---

/// Escape for embedding in a single-quoted JS string literal.
pub fn js_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '<' => out.push_str("\\x3C"),
            _ => out.push(c),
        }
    }
    out
}

pub fn exists_js(selector: &str) -> String {
    format!("!!document.querySelector('{}')", js_str(selector))
}

pub fn click_js(selector: &str) -> String {
    format!(
        "(() => {{ const e = document.querySelector('{}');
          if (!e) return false;
          e.scrollIntoView({{block:'center'}}); e.click(); return true; }})()",
        js_str(selector)
    )
}

/// Sets the value and fires input+change, because frameworks ignore a value
/// assignment that arrives without events.
pub fn fill_js(selector: &str, text: &str) -> String {
    format!(
        "(() => {{ const e = document.querySelector('{}');
          if (!e) return false;
          e.focus(); e.value = '{}';
          e.dispatchEvent(new Event('input', {{bubbles:true}}));
          e.dispatchEvent(new Event('change', {{bubbles:true}}));
          return true; }})()",
        js_str(selector),
        js_str(text)
    )
}

/// Find the debugger websocket URL from Chrome's /json/list endpoint.
pub fn ws_url_from_targets(json_body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(json_body).ok()?;
    v.as_array()?
        .iter()
        .filter(|t| t.get("type").and_then(Value::as_str) == Some("page"))
        .find_map(|t| t.get("webSocketDebuggerUrl").and_then(Value::as_str))
        .map(str::to_string)
}
