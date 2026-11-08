//! FreeLLMAPI's bounded local-gateway adapter.
//!
//! The gateway remains opt-in and loopback-only from Atlas's perspective.

use crate::brain::{dig, Llm, LlmConfig, LongReply};
use crate::execution::ExecutionDescriptor;
use crate::tools::{ExternalTool, Vars};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

const MAX_LOOPBACK_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const FREELLMAPI_FAILURES_TO_OPEN: u32 = 1;
const FREELLMAPI_OPEN_MS: u64 = 30_000;
const FREELLMAPI_MAX_OPEN_MS: u64 = 600_000;

#[derive(Debug)]
struct FreeLlmApiGate {
    breaker: crate::ratelimit::Breaker,
}

impl Default for FreeLlmApiGate {
    fn default() -> Self {
        Self {
            breaker: crate::ratelimit::Breaker::new(
                FREELLMAPI_FAILURES_TO_OPEN,
                FREELLMAPI_OPEN_MS,
                FREELLMAPI_MAX_OPEN_MS,
            ),
        }
    }
}

fn freellmapi_gates() -> &'static std::sync::Mutex<HashMap<String, FreeLlmApiGate>> {
    static GATES: std::sync::OnceLock<std::sync::Mutex<HashMap<String, FreeLlmApiGate>>> =
        std::sync::OnceLock::new();
    GATES.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn freellmapi_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn freellmapi_gate_allows(endpoint: &str) -> bool {
    freellmapi_gates()
        .lock()
        .or_else(crate::crash::unpoison)
        .map(|mut gates| {
            gates
                .entry(endpoint.to_string())
                .or_default()
                .breaker
                .allow(freellmapi_now_ms())
        })
        .unwrap_or(true)
}

fn freellmapi_gate_result(endpoint: &str, success: bool) {
    if let Ok(mut gates) = freellmapi_gates().lock().or_else(crate::crash::unpoison) {
        let now = freellmapi_now_ms();
        let gate = gates.entry(endpoint.to_string()).or_default();
        if success {
            gate.breaker.success();
        } else {
            gate.breaker.failure(now);
        }
    }
}

fn freellmapi_gate_open(endpoint: &str) -> bool {
    freellmapi_gates()
        .lock()
        .or_else(crate::crash::unpoison)
        .map(|mut gates| {
            let gate = gates.entry(endpoint.to_string()).or_default();
            matches!(
                gate.breaker.state(freellmapi_now_ms()),
                crate::ratelimit::State::Open { .. }
            )
        })
        .unwrap_or(false)
}

/// Optional local FreeLLMAPI gateway for background work.
///
/// This is intentionally a gateway configuration, not a second provider
/// registry. FreeLLMAPI owns provider routing and failover; Atlas owns the
/// delegation boundary, redaction, local verification, and receipts. It is
/// disabled by default because a loopback endpoint may still forward prompts
/// to remote free-tier providers.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FreeLlmApiConfig {
    /// Explicit opt-in to remote-capable routing through the local gateway.
    pub enabled: bool,
    /// The OpenAI-compatible chat endpoint. Keep this loopback-only so a
    /// config typo cannot turn this adapter into an arbitrary remote caller.
    pub endpoint: String,
    /// Model id understood by the gateway's catalog.
    pub model: String,
    /// Vault entry containing FreeLLMAPI's unified bearer token.
    pub token_vault: String,
    /// Re-check the delegated research result with Atlas's local model.
    #[serde(default = "FreeLlmApiConfig::verify_default")]
    pub verify: bool,
}

/// Remove the gateway bearer token before shared variables are handed to a
/// research fetch/search tool. The LLM adapter keeps its own token, but tool
/// templates are a separate execution boundary and must not be able to expand
/// `{token}` accidentally.
pub(crate) fn remove_gateway_token_from_tool_vars(vars: &mut Vars) {
    vars.remove("token");
}

/// Let the gateway choose from its current healthy fallback chain. A pinned
/// provider model can disappear from a free catalog or become exhausted while
/// the router still has another usable endpoint.
pub const DEFAULT_FREELLMAPI_MODEL: &str = "auto";

impl Default for FreeLlmApiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            // FreeLLMAPI's Windows desktop app binds its local router to
            // 31415. A source/Docker install can still override this in
            // tools.yaml (commonly 3001); keep the shipped default aligned
            // with the desktop runtime already present on Eric's laptop.
            endpoint: "http://127.0.0.1:31415/v1/chat/completions".into(),
            model: DEFAULT_FREELLMAPI_MODEL.into(),
            token_vault: "freellmapi unified key".into(),
            verify: true,
        }
    }
}

impl FreeLlmApiConfig {
    fn verify_default() -> bool {
        true
    }

    /// A plain-language setup contract used by diagnostics and handoffs.
    pub const fn setup_steps() -> [&'static str; 5] {
        [
            "Keep FreeLLMAPI bound to a numeric loopback address; it is not a multi-tenant production service.",
            "Create or copy its unified API key without putting it in Atlas YAML.",
            "Store that key in Atlas's protected vault under the configured entry name.",
            "Choose a catalog model and explicitly enable FreeLLMAPI background delegation.",
            "Keep local verification enabled; routed provider/model details remain unknown unless proven.",
        ]
    }

    /// Returns the OpenAI-shaped adapter only after the endpoint is proven to
    /// be a loopback HTTP URL and the model identity is bounded.
    pub fn llm_config(&self) -> Result<LlmConfig, String> {
        let endpoint = self.endpoint.trim();
        if endpoint.is_empty()
            || endpoint.chars().any(char::is_control)
            || endpoint.contains(' ')
            || !is_loopback_endpoint(endpoint)
        {
            return Err("FreeLLMAPI endpoint must be a loopback HTTP URL".into());
        }
        let model = self.model.trim();
        if model.is_empty()
            || model.len() > 256
            || model
                .chars()
                .any(|c| c.is_control() || matches!(c, '"' | '\\'))
        {
            return Err("FreeLLMAPI model must be a bounded, non-empty id".into());
        }
        if self.token_vault.trim().is_empty() {
            return Err("FreeLLMAPI needs a vault entry for its unified key".into());
        }

        let model_json = serde_json::to_string(model)
            .map_err(|_| "FreeLLMAPI model could not be encoded".to_string())?;
        Ok(LlmConfig {
            tool: ExternalTool {
                command: "curl".into(),
                args: [
                    "-s",
                    "-S",
                    "--noproxy",
                    "*",
                    "-m",
                    "180",
                    "-X",
                    "POST",
                    endpoint,
                    "-H",
                    "Content-Type: application/json",
                    "-H",
                    "Authorization: Bearer {token}",
                    "-d",
                    "@-",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                stdin_text: true,
                timeout_secs: 190,
                ..ExternalTool::default()
            },
            request: format!(
                "{{\"model\":{model_json},\"messages\":[{{\"role\":\"system\",\"content\":{{system}}}},{{\"role\":\"user\",\"content\":{{user}}}}],\"stream\":false}}"
            ),
            response_path: "choices.0.message.content".into(),
            vision_request: None,
        })
    }

    /// The gateway is remote-capable even though the process is loopback.
    /// This explicit readiness gate prevents accidental use while the vault
    /// is locked or the config is incomplete.
    pub fn readiness(&self, token_present: bool) -> crate::online::Readiness {
        if !self.enabled {
            return crate::online::Readiness::Blocked(
                "FreeLLMAPI delegation is switched off".into(),
            );
        }
        if let Err(why) = self.llm_config() {
            return crate::online::Readiness::Blocked(why);
        }
        if !token_present {
            return crate::online::Readiness::Blocked(format!(
                "the FreeLLMAPI key isn't in the vault under \"{}\"",
                self.token_vault
            ));
        }
        if freellmapi_gate_open(self.endpoint.trim()) {
            return crate::online::Readiness::Blocked(
                "the FreeLLMAPI gateway is cooling down after a recent failure".into(),
            );
        }
        crate::online::Readiness::Ready
    }
}

/// Provider metadata returned by FreeLLMAPI. It is deliberately metadata only:
/// no prompt, completion, or credential is retained here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FreeLlmApiMetadata {
    pub routed_via: Option<String>,
    pub fallback_attempts: Option<String>,
    pub fallback_trail: Option<String>,
}

/// A loopback adapter around the LLM contract. FreeLLMAPI is remote-capable,
/// but its bearer key never needs to enter a child-process argument list:
/// Atlas writes the request directly to the loopback socket and keeps only
/// non-sensitive routing headers for provenance.
pub struct FreeLlmApiLlm {
    endpoint: String,
    model: String,
    token: String,
    last_metadata: std::sync::Mutex<Option<FreeLlmApiMetadata>>,
}

impl FreeLlmApiLlm {
    pub fn new(cfg: &FreeLlmApiConfig, vars: Vars) -> Result<Self, String> {
        cfg.llm_config()?;
        let model = cfg.model.trim().to_string();
        let endpoint = cfg.endpoint.trim().to_string();
        if !endpoint.starts_with("http://") {
            return Err("FreeLLMAPI native transport requires loopback HTTP".into());
        }
        let token = vars
            .get("token")
            .filter(|token| !token.trim().is_empty())
            .cloned()
            .ok_or_else(|| {
                "FreeLLMAPI unified key is missing from the vault handoff".to_string()
            })?;
        Ok(Self {
            endpoint,
            model,
            token,
            last_metadata: std::sync::Mutex::new(None),
        })
    }

    /// The latest non-sensitive routing metadata, if a request has completed.
    pub fn last_metadata(&self) -> Option<FreeLlmApiMetadata> {
        self.last_metadata
            .lock()
            .or_else(crate::crash::unpoison)
            .ok()
            .and_then(|metadata| metadata.clone())
    }

    fn request(
        &self,
        system: &str,
        user: &str,
        max_tokens: Option<u32>,
    ) -> crate::error::Result<Value> {
        self.request_until(system, user, max_tokens, &|| true)
    }

    fn request_until(
        &self,
        system: &str,
        user: &str,
        max_tokens: Option<u32>,
        keep_going: &dyn Fn() -> bool,
    ) -> crate::error::Result<Value> {
        if !keep_going() {
            return Err(crate::error::AtlasError::Platform(
                "FreeLLMAPI request stopped".into(),
            ));
        }
        if !freellmapi_gate_allows(&self.endpoint) {
            return Err(crate::error::AtlasError::Platform(
                "FreeLLMAPI gateway is cooling down after a recent failure; use the local fallback"
                    .into(),
            ));
        }
        let result = self.request_once_until(system, user, max_tokens, keep_going);
        if result.is_ok()
            || !result
                .as_ref()
                .err()
                .is_some_and(freellmapi_error_is_cancellation)
        {
            freellmapi_gate_result(&self.endpoint, result.is_ok());
        }
        result
    }

    fn request_once_until(
        &self,
        system: &str,
        user: &str,
        max_tokens: Option<u32>,
        keep_going: &dyn Fn() -> bool,
    ) -> crate::error::Result<Value> {
        let mut request = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "stream": false
        });
        if let Some(max_tokens) = max_tokens {
            request["max_tokens"] = serde_json::json!(max_tokens);
        }
        let body = serde_json::to_string(&request)
            .map_err(|error| crate::error::AtlasError::Platform(error.to_string()))?;
        let response = post_loopback_json_until(&self.endpoint, &self.token, &body, keep_going)?;
        if !(200..300).contains(&response.status) {
            return Err(crate::error::AtlasError::Platform(format!(
                "FreeLLMAPI returned HTTP {}: {}",
                response.status,
                truncate_response(&response.body)
            )));
        }
        if response.metadata.routed_via.is_none() {
            return Err(crate::error::AtlasError::Platform(
                "FreeLLMAPI response lacked X-Routed-Via provenance".into(),
            ));
        }
        if let Ok(mut metadata) = self.last_metadata.lock().or_else(crate::crash::unpoison) {
            *metadata = Some(response.metadata);
        }
        serde_json::from_str(&response.body).map_err(|error| {
            crate::error::AtlasError::Platform(format!(
                "FreeLLMAPI returned non-JSON: {error}. Got: {}",
                truncate_response(&response.body)
            ))
        })
    }
}

impl Llm for FreeLlmApiLlm {
    fn execution_descriptor(&self) -> ExecutionDescriptor {
        ExecutionDescriptor::new(
            "freellmapi-gateway",
            &self.model,
            "openai-compatible-gateway",
            "unknown",
            "remote-via-loopback-gateway",
            "unknown",
        )
        .unwrap_or_default()
    }

    fn execution_metadata(&self) -> Option<crate::execution::ExecutionMetadata> {
        self.last_metadata()
            .map(|metadata| crate::execution::ExecutionMetadata {
                routed_via: metadata.routed_via,
                fallback_attempts: metadata.fallback_attempts,
                fallback_trail: metadata.fallback_trail,
            })
    }

    fn supports_bounded_chat(&self) -> bool {
        true
    }

    fn chat_until(
        &self,
        req: &crate::brain::ChatRequest,
        on_text: &mut dyn FnMut(&str) -> bool,
        keep_going: &dyn Fn() -> bool,
    ) -> crate::error::Result<crate::brain::ChatReply> {
        let (system, user) = crate::brain::flatten_messages(&req.messages);
        let response = self.request_until(
            &system,
            &user,
            (req.max_tokens > 0).then_some(req.max_tokens),
            keep_going,
        )?;
        let text = dig(&response, "choices.0.message.content")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                crate::error::AtlasError::Platform("FreeLLMAPI response had no text".into())
            })?;
        if !on_text(&text) || !keep_going() {
            return Err(crate::error::AtlasError::Platform(
                "FreeLLMAPI response stopped".into(),
            ));
        }
        let mut reply = crate::brain::ChatReply::from_text(&text);
        reply.execution = self.execution_descriptor();
        Ok(reply)
    }

    fn complete(&self, system: &str, user: &str) -> crate::error::Result<String> {
        let response = self.request(system, user, None)?;
        dig(&response, "choices.0.message.content")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                crate::error::AtlasError::Platform("FreeLLMAPI response had no text".into())
            })
    }

    fn complete_long(
        &self,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> crate::error::Result<LongReply> {
        let response = self.request(system, user, Some(max_tokens))?;
        let text = dig(&response, "choices.0.message.content")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                crate::error::AtlasError::Platform("FreeLLMAPI response had no text".into())
            })?;
        Ok(LongReply {
            text,
            cut_off: dig(&response, "choices.0.finish_reason").and_then(Value::as_str)
                == Some("length"),
        })
    }

    fn context_tokens(&self) -> Option<u32> {
        Some(32_000)
    }
}

struct LoopbackResponse {
    status: u16,
    body: String,
    metadata: FreeLlmApiMetadata,
}

fn post_loopback_json_until(
    endpoint: &str,
    token: &str,
    body: &str,
    keep_going: &dyn Fn() -> bool,
) -> crate::error::Result<LoopbackResponse> {
    let (authority, path) = endpoint
        .strip_prefix("http://")
        .and_then(|rest| rest.split_once('/'))
        .ok_or_else(|| {
            crate::error::AtlasError::Platform("FreeLLMAPI endpoint must include a path".into())
        })?;
    if path.is_empty() {
        return Err(crate::error::AtlasError::Platform(
            "FreeLLMAPI endpoint path must not be empty".into(),
        ));
    }
    let (host, port) = split_loopback_authority(authority)?;
    let connect_host = host.trim_matches(['[', ']']);
    let ip = connect_host.parse::<IpAddr>().map_err(|error| {
        crate::error::AtlasError::Platform(format!("FreeLLMAPI host is not numeric: {error}"))
    })?;
    let address = SocketAddr::new(ip, port);
    let mut stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(5)).map_err(|error| {
            crate::error::AtlasError::Platform(format!("FreeLLMAPI connection failed: {error}"))
        })?;
    let timeout = Duration::from_secs(190);
    stream
        .set_nonblocking(true)
        .map_err(|error| crate::error::AtlasError::Platform(error.to_string()))?;
    let host_header = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    if token.chars().any(char::is_control) {
        return Err(crate::error::AtlasError::Platform(
            "FreeLLMAPI unified key contains control characters".into(),
        ));
    }
    let request = format!(
        "POST /{path} HTTP/1.1\r\nHost: {host_header}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let deadline = Instant::now() + timeout;
    write_loopback_until(&mut stream, request.as_bytes(), keep_going, deadline)?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if !keep_going() {
            return Err(crate::error::AtlasError::Platform(
                "FreeLLMAPI response stopped".into(),
            ));
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                if raw.len().saturating_add(read) > MAX_LOOPBACK_RESPONSE_BYTES {
                    return Err(crate::error::AtlasError::Platform(
                        "FreeLLMAPI response exceeded the 8 MiB limit".into(),
                    ));
                }
                raw.extend_from_slice(&chunk[..read]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(crate::error::AtlasError::Platform(
                        "FreeLLMAPI response timed out".into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                return Err(crate::error::AtlasError::Platform(format!(
                    "FreeLLMAPI response failed: {error}"
                )));
            }
        }
    }
    parse_loopback_response(&raw)
}

fn freellmapi_error_is_cancellation(error: &crate::error::AtlasError) -> bool {
    matches!(
        error,
        crate::error::AtlasError::Platform(message)
            if message.contains("FreeLLMAPI request stopped")
                || message.contains("FreeLLMAPI response stopped")
    )
}

fn write_loopback_until(
    stream: &mut TcpStream,
    request: &[u8],
    keep_going: &dyn Fn() -> bool,
    deadline: Instant,
) -> crate::error::Result<()> {
    let mut written = 0usize;
    while written < request.len() {
        if !keep_going() {
            return Err(crate::error::AtlasError::Platform(
                "FreeLLMAPI request stopped".into(),
            ));
        }
        match stream.write(&request[written..]) {
            Ok(0) => {
                return Err(crate::error::AtlasError::Platform(
                    "FreeLLMAPI request wrote zero bytes".into(),
                ));
            }
            Ok(count) => written += count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(crate::error::AtlasError::Platform(
                        "FreeLLMAPI request timed out".into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                return Err(crate::error::AtlasError::Platform(format!(
                    "FreeLLMAPI request failed: {error}"
                )));
            }
        }
    }
    Ok(())
}

fn split_loopback_authority(authority: &str) -> crate::error::Result<(&str, u16)> {
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| crate::error::AtlasError::Platform("invalid FreeLLMAPI host".into()))?;
        let host = &rest[..end];
        let port = rest[end + 1..]
            .strip_prefix(':')
            .unwrap_or("80")
            .parse()
            .map_err(|_| crate::error::AtlasError::Platform("invalid FreeLLMAPI port".into()))?;
        (host, port)
    } else {
        let (host, port) = authority.rsplit_once(':').unwrap_or((authority, "80"));
        let port = port
            .parse()
            .map_err(|_| crate::error::AtlasError::Platform("invalid FreeLLMAPI port".into()))?;
        (host, port)
    };
    if !matches!(host.to_ascii_lowercase().as_str(), "127.0.0.1" | "::1") {
        return Err(crate::error::AtlasError::Platform(
            "FreeLLMAPI transport is loopback-only".into(),
        ));
    }
    Ok((host, port))
}

fn parse_loopback_response(raw: &[u8]) -> crate::error::Result<LoopbackResponse> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            crate::error::AtlasError::Platform("FreeLLMAPI response had no headers".into())
        })?;
    let header_text = String::from_utf8_lossy(&raw[..split]);
    let mut lines = header_text.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            crate::error::AtlasError::Platform("FreeLLMAPI response had no HTTP status".into())
        })?;
    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let body = &raw[split + 4..];
    let body = if headers
        .get("transfer-encoding")
        .is_some_and(|value| value.to_ascii_lowercase().contains("chunked"))
    {
        decode_chunked(body)?
    } else {
        body.to_vec()
    };
    let body = String::from_utf8(body).map_err(|error| {
        crate::error::AtlasError::Platform(format!("FreeLLMAPI response was not UTF-8: {error}"))
    })?;
    Ok(LoopbackResponse {
        status,
        body,
        metadata: FreeLlmApiMetadata {
            routed_via: headers.get("x-routed-via").cloned(),
            fallback_attempts: headers.get("x-fallback-attempts").cloned(),
            fallback_trail: headers.get("x-fallback-trail").cloned(),
        },
    })
}

fn decode_chunked(mut bytes: &[u8]) -> crate::error::Result<Vec<u8>> {
    let mut decoded = Vec::new();
    loop {
        let end = bytes
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| {
                crate::error::AtlasError::Platform("malformed chunked response".into())
            })?;
        let size_text =
            std::str::from_utf8(bytes[..end].split(|b| *b == b';').next().unwrap_or(&[]))
                .map_err(|_| crate::error::AtlasError::Platform("malformed chunk size".into()))?;
        let size = usize::from_str_radix(size_text.trim(), 16)
            .map_err(|_| crate::error::AtlasError::Platform("malformed chunk size".into()))?;
        let required = size
            .checked_add(2)
            .ok_or_else(|| crate::error::AtlasError::Platform("chunk size overflow".into()))?;
        bytes = &bytes[end + 2..];
        if size == 0 {
            return Ok(decoded);
        }
        if bytes.len() < required || &bytes[size..required] != b"\r\n" {
            return Err(crate::error::AtlasError::Platform(
                "truncated chunked response".into(),
            ));
        }
        decoded.extend_from_slice(&bytes[..size]);
        bytes = &bytes[size + 2..];
    }
}

fn truncate_response(body: &str) -> String {
    body.chars().take(240).collect()
}

fn is_loopback_endpoint(endpoint: &str) -> bool {
    let Some((scheme, rest)) = endpoint.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let host = if let Some(end) = authority.find(']') {
        &authority[..=end]
    } else {
        authority.split(':').next().unwrap_or("")
    };
    matches!(
        host.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "[::1]" | "::1"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn free_cfg() -> FreeLlmApiConfig {
        FreeLlmApiConfig {
            enabled: true,
            endpoint: "http://127.0.0.1:3001/v1/chat/completions".into(),
            model: "free-model".into(),
            token_vault: "freellmapi unified key".into(),
            verify: true,
        }
    }

    #[test]
    fn freellmapi_requires_explicit_opt_in_and_vault_key() {
        let mut c = free_cfg();
        c.enabled = false;
        assert_eq!(
            c.readiness(true).why(),
            "FreeLLMAPI delegation is switched off"
        );
        c.enabled = true;
        assert!(c.readiness(false).why().contains("freellmapi unified key"));
        assert!(c.readiness(true).ready_now());
    }

    #[test]
    fn freellmapi_gate_opens_after_failure_and_recovers_after_cooldown() {
        let mut gate = FreeLlmApiGate::default();
        assert!(gate.breaker.allow(0));
        gate.breaker.failure(0);
        assert!(!gate.breaker.allow(FREELLMAPI_OPEN_MS - 1));
        assert!(gate.breaker.allow(FREELLMAPI_OPEN_MS));
        gate.breaker.success();
        assert!(gate.breaker.allow(FREELLMAPI_OPEN_MS + 1));
    }

    #[test]
    fn freellmapi_accepts_loopback_only_and_never_embeds_the_key() {
        let c = free_cfg();
        let cfg = c.llm_config().unwrap();
        assert!(cfg
            .tool
            .args
            .iter()
            .any(|arg| arg == "Authorization: Bearer {token}"));
        assert!(!cfg.request.contains("freellmapi unified key"));
        let mut vars = Vars::new();
        vars.insert("token".into(), "test-unified-key".into());
        let llm = FreeLlmApiLlm::new(&c, vars).unwrap();
        assert_eq!(
            llm.execution_descriptor().network,
            "remote-via-loopback-gateway"
        );

        let mut public = c;
        public.endpoint = "https://example.test/v1/chat/completions".into();
        assert!(public.llm_config().is_err());
        public.endpoint = "http://localhost:3001/v1/chat/completions".into();
        assert!(public.llm_config().is_err());
    }

    #[test]
    fn gateway_token_is_not_available_to_research_tools() {
        let mut vars = Vars::new();
        vars.insert("token".into(), "free-secret".into());
        vars.insert("query".into(), "safe query".into());

        remove_gateway_token_from_tool_vars(&mut vars);

        assert!(!vars.contains_key("token"));
        assert_eq!(vars.get("query").map(String::as_str), Some("safe query"));
    }

    #[test]
    fn native_loopback_transport_preserves_path_and_records_provenance() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let body = r#"{"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]}"#;
            write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nX-Routed-Via: test/provider\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            String::from_utf8(request).unwrap()
        });

        let mut cfg = free_cfg();
        cfg.endpoint = format!("http://127.0.0.1:{port}/v1/chat/completions");
        let mut vars = Vars::new();
        vars.insert("token".into(), "test-unified-key".into());
        let llm = FreeLlmApiLlm::new(&cfg, vars).unwrap();
        assert_eq!(llm.complete("system", "user").unwrap(), "ok");
        assert_eq!(
            llm.last_metadata().unwrap().routed_via.as_deref(),
            Some("test/provider")
        );
        let request = server.join().unwrap();
        assert!(
            request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"),
            "captured request: {request:?}"
        );
        assert!(request.contains("Authorization: Bearer test-unified-key\r\n"));
    }

    #[test]
    fn native_transport_rejects_https_and_resolver_names() {
        let mut cfg = free_cfg();
        cfg.endpoint = "https://127.0.0.1:31415/v1/chat/completions".into();
        let mut vars = Vars::new();
        vars.insert("token".into(), "test-unified-key".into());
        assert!(FreeLlmApiLlm::new(&cfg, vars).is_err());
        assert!(split_loopback_authority("localhost:31415").is_err());
    }

    #[test]
    fn poisoned_metadata_lock_still_returns_routing_provenance() {
        let llm = FreeLlmApiLlm::new(&free_cfg(), {
            let mut vars = Vars::new();
            vars.insert("token".into(), "test-unified-key".into());
            vars
        })
        .unwrap();
        *llm.last_metadata.lock().unwrap() = Some(FreeLlmApiMetadata {
            routed_via: Some("test/provider".into()),
            ..Default::default()
        });
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = llm.last_metadata.lock().unwrap();
            panic!("poison the metadata lock");
        }));
        assert_eq!(
            llm.last_metadata().and_then(|metadata| metadata.routed_via),
            Some("test/provider".into())
        );
    }

    #[test]
    fn bounded_chat_stops_before_connect_without_opening_the_breaker() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut cfg = free_cfg();
        cfg.endpoint = format!("http://127.0.0.1:{port}/v1/chat/completions");
        let mut vars = Vars::new();
        vars.insert("token".into(), "test-unified-key".into());
        let llm = FreeLlmApiLlm::new(&cfg, vars).unwrap();
        let request = crate::brain::ChatRequest {
            messages: vec![crate::brain::Msg::user("cancelled")],
            max_tokens: 32,
            ..Default::default()
        };
        let mut seen = false;
        let result = llm.chat_until(
            &request,
            &mut |_| {
                seen = true;
                true
            },
            &|| false,
        );
        assert!(result.is_err());
        assert!(!seen);
        assert!(!freellmapi_gate_open(
            "http://127.0.0.1:1/v1/chat/completions"
        ));
        listener
            .set_nonblocking(true)
            .expect("inspect cancellation listener");
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }
}
