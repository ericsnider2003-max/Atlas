// Scratch: rank a talking model on tool choice for sentences the phrases miss.
use std::sync::Arc;
const ROUTING: &[(&str, &str)] = &[
    ("ugh, I can never remember what meetings I've got on friday", "agenda"),
    ("is anything on for me tomorrow afternoon", "agenda"),
    ("the fan on this thing is going crazy, is something hogging it", "machine_health"),
    ("how's the battery and memory holding up", "machine_health"),
    ("I want to know about the history of the eiffel tower, can you find out", "research"),
    ("keep in mind that my passport expires in june", "capture"),
    ("pull up spotify for me", "open_app"),
    ("show me what jobs you've found", "opportunities"),
    ("any decent remote gigs going for video editors", "opportunities"),
    ("I can't find my tax return from last year", "find_file"),
    ("throw a picture together of a cabin by a lake", "make_picture"),
    ("what's the capital of australia", "-"),
    ("I'm feeling a bit flat today", "-"),
    ("explain how a heat pump works in two sentences", "-"),
    ("what should I make for dinner with chicken and rice", "-"),
    ("tell me a fun fact about octopuses", "-"),
    ("how do I boil an egg properly", "-"),
];
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = std::path::PathBuf::from(&a[1]);
    let port: u16 = a.get(2).and_then(|p| p.parse().ok()).unwrap_or(8091);
    let cfg = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let tc = cfg.tools.as_ref().unwrap();
    let reg = atlas::models::Registry::scan(path.parent().unwrap());
    let model = reg.models.iter().find(|m| m.path.file_name() == path.file_name()).expect("model");
    let mut mcfg = tc.models.clone();
    mcfg.port = port;
    let lc = atlas::models::llm_config_for(model, &mcfg, &atlas::models::server_post());
    let llm = Arc::new(atlas::brain::ShellLlm { cfg: lc, vars: tc.vars.clone() }) as Arc<dyn atlas::brain::Llm>;
    let parser = atlas::intent::Parser::new(&cfg.commands);
    let plat = atlas::platform::mock::MockPlatform::new(vec![atlas::platform::Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let (mut right, mut decided) = (0, 0);
    let mut ms_all = Vec::new();
    for (i, (said, want)) in ROUTING.iter().enumerate() {
        let parsed = atlas::session::kind_of(&parser.parse(said)).to_string();
        let store = std::env::temp_dir().join(format!("atlas-rank-{}-{i}", std::process::id()));
        let _ = std::fs::create_dir_all(&store);
        let mut d = atlas::daemon::Daemon::new(&cfg, &plat, Some(llm.clone()), atlas::store::Store::new(store.clone()), atlas::proactive::Proactive::new(atlas::proactive::ProactiveConfig::default()));
        d.rehearsal = true;
        let t = std::time::Instant::now();
        let reply = d.turn(said, atlas::store::now());
        let ms = t.elapsed().as_millis();
        ms_all.push(ms);
        let reached = d.last_reached().unwrap_or_else(|| parsed.clone());
        let want_kind = if *want == "-" { "-".to_string() } else {
            atlas::intent::from_tool(want, &serde_json::json!({"arg": "x"}), said).map(|i| atlas::session::kind_of(&i).to_string()).unwrap_or(want.to_string())
        };
        let by_model = parsed == "unknown" || parsed == "chat";
        let ok = if want_kind == "-" { ["unknown", "chat", "converse", "say"].contains(&reached.as_str()) } else { reached == want_kind || (want_kind == "open_app" && reached == "launch") };
        if by_model { decided += 1; if ok { right += 1; } }
        println!("{} [{}ms] parsed={parsed} reached={reached} want={want_kind} :: {said}\n    -> {}", if ok {"OK  "} else {"MISS"}, ms, reply.replace('\n', " ").chars().take(160).collect::<String>());
        let _ = std::fs::remove_dir_all(&store);
    }
    ms_all.sort();
    println!("ROUTING {}: {right}/{decided} right among those the model decided; median {} ms", model.id, ms_all[ms_all.len()/2]);
}
