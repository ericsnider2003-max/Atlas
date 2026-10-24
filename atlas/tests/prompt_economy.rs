//! What the local model is made to read (Plan G, 24 Oct 2026: prompt
//! economy). Charter s7: a useful answer within 20 seconds on the device it
//! runs on -- and on a small local model, every word of prompt is time read
//! before the first word of answer. This measures what one ordinary question
//! costs, part by part, and holds that cost to a budget so it can't quietly
//! grow; trimming it is a measured change made against this baseline.

use atlas::brain::{ChatReply, ChatRequest, Llm, Role};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// A model that answers plainly and keeps what it was asked.
struct Records(Arc<Mutex<Vec<ChatRequest>>>);
impl Llm for Records {
    fn complete(&self, _s: &str, _u: &str) -> atlas::error::Result<String> {
        Ok("Sunlight scatters off the air, and blue scatters most.".into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, _on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        self.0.lock().unwrap().push(req.clone());
        Ok(ChatReply {
            text: "Sunlight scatters off the air, and blue scatters most.".into(),
            tool_calls: Vec::new(),
        })
    }
}

/// Characters per token, roughly, for English and JSON alike: an estimate,
/// labelled as one wherever it is printed.
const CHARS_PER_TOKEN: usize = 4;

/// The budget for one ordinary question's first request, in characters:
/// the baseline measured on 24 Oct 2026 plus 10%. Raising it is a decision,
/// not a drift -- say what was added and why.
const BUDGET_CHARS: usize = PROMPT_BASELINE_CHARS * 11 / 10;
/// Measured 24 Oct 2026 on a fresh profile: system 1,536, user 366 (346 of
/// them framing around a 20-character question), one tool 336. Found while
/// measuring: the date is said twice (system and user), and the length rule
/// contradicts itself -- "one to three short sentences" in the system
/// prompt, "up to about 8 sentences" in the user message. About 80 tokens
/// of every turn; trimming them waits on a quality check (charter s5).
/// 24 Oct 2026: the length rule is now one sentence in one place
/// (`persona::LENGTH_RULE`), and this baseline is unchanged: it holds.
const PROMPT_BASELINE_CHARS: usize = 2238;

fn measured() -> (usize, Vec<(String, usize)>) {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let root = std::env::temp_dir().join(format!("atlas-prompt-economy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let llm: Arc<dyn Llm> = Arc::new(Records(seen.clone()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(&root), Proactive::new(ProactiveConfig::default()));
    d.turn("why is the sky blue?", atlas::store::now());
    let reqs = seen.lock().unwrap().clone();
    let first = reqs.first().expect("the question reached the model").clone();
    let mut parts: Vec<(String, usize)> = Vec::new();
    for (i, m) in first.messages.iter().enumerate() {
        let role = match m.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        parts.push((format!("message {i} ({role})"), m.content.len()));
    }
    if std::env::var("ATLAS_SHOW_PROMPT").is_ok() {
        for m in &first.messages {
            eprintln!("---- {:?}
{}", m.role, m.content);
        }
    }
    let tools: usize = first.tools.iter().map(|t| t.to_string().len()).sum();
    parts.push((format!("{} tool definitions", first.tools.len()), tools));
    let total = parts.iter().map(|(_, n)| n).sum();
    (total, parts)
}

#[test]
fn one_ordinary_question_costs_what_it_costs_and_no_more() {
    let (total, parts) = measured();
    eprintln!("PROMPT one question: {total} chars, about {} tokens (estimate, {CHARS_PER_TOKEN} chars/token)", total / CHARS_PER_TOKEN);
    for (what, n) in &parts {
        eprintln!("PROMPT   {what}: {n} chars, about {} tokens", n / CHARS_PER_TOKEN);
    }
    assert!(
        total <= BUDGET_CHARS,
        "one question now costs {total} chars, over the budget of {BUDGET_CHARS}: say what was added and why"
    );
}

/// 24 Oct 2026 (hand-off from Plan G): the length rule was said twice and
/// disagreed -- "one to three short sentences" in the system prompt, "up to
/// about 8 sentences" in the user message. It is now one sentence in one
/// place (`persona::LENGTH_RULE`). Whatever a model is handed, that rule
/// appears once, and no other text names a count of sentences.
#[test]
fn the_prompts_never_hold_two_length_rules() {
    use atlas::persona::{Persona, LENGTH_RULE};
    use atlas::register::Register;
    let mut texts: Vec<(String, String)> = Vec::new();
    // What the model gets on one ordinary question.
    {
        let c = Config::load(Path::new("config")).unwrap();
        let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
        let root = std::env::temp_dir().join(format!("atlas-prompt-length-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let llm: Arc<dyn Llm> = Arc::new(Records(seen.clone()));
        let mut d = Daemon::new(&c, &p, Some(llm), Store::new(&root), Proactive::new(ProactiveConfig::default()));
        d.turn("why is the sky blue?", atlas::store::now());
        let first = seen.lock().unwrap().first().cloned().expect("reached the model");
        let all: Vec<String> = first.messages.iter().map(|m| m.content.clone()).collect();
        texts.push(("the first request".into(), all.join("\n")));
    }
    // The prompts built for each register, and the per-turn line.
    // The standard persona and the shipped one (config: eight).
    for max in [3usize, 8] {
        let persona = Persona { max_spoken_sentences: max, ..Persona::default() };
        for r in [Register::Working, Register::Chatting, Register::AboutAtlas, Register::Rough] {
            let both = format!("{}\n{}", persona.prompt_for(r), persona.for_this_turn_on(r, 3, "why is the sky blue?", false));
            texts.push((format!("{r:?} prompt plus turn line at {max}"), both));
        }
    }
    let persona = Persona::default();
    for (what, t) in &texts {
        assert!(t.matches(LENGTH_RULE).count() <= 1, "{what}: the length rule is said more than once");
        let rest = t.replace(LENGTH_RULE, "");
        let lower = rest.to_lowercase();
        assert!(
            !lower.contains("sentences"),
            "{what}: a second length rule outside LENGTH_RULE: {:?}",
            lower.split("sentences").next().map(|s| s.chars().rev().take(60).collect::<String>().chars().rev().collect::<String>())
        );
    }
    // The system prompt and the character both carry the one rule.
    assert!(persona.system_prompt().contains(LENGTH_RULE));
    assert!(persona.character().contains(LENGTH_RULE));
}
