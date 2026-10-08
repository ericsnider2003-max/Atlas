//! The Windows Firewall rule for the door your own devices use (gap AJ, 8.9).
//!
//! Friends reach Atlas through Tor, which only ever connects *out*, so no
//! rule is needed for them. But the ordinary door (`server::SignalListener`)
//! also listens beyond this machine, for your own phone over Tailscale and a
//! friend on the same wifi. The first time something connects to it, Windows
//! stops and asks -- and a "Cancel" there quietly makes a *block* rule that
//! stays. So the setup adds the rule itself, once, with Windows' own tool
//! (`netsh advfirewall`), and asks for your permission to do it the ordinary
//! way (the Windows "allow changes" prompt), because a firewall rule is an
//! administrator's change.
//!
//! The rule is as narrow as the door's job:
//! - **only Atlas** (`program=` this exe) -- not a port anyone could reuse;
//! - **only incoming TCP**;
//! - **only on private and work networks** (`profile=private,domain`): on a
//!   café's wifi, which Windows calls public, nothing gets in;
//! - **only from your own networks**: this subnet, Tailscale's addresses
//!   (100.64.0.0/10 and fd7a:115c:a1e0::/48), which is what the door itself
//!   also checks (`onion::is_local_origin`).
//!
//! Nothing here runs anywhere but Windows; elsewhere it says there's nothing
//! to do.

use std::path::Path;

/// The rule's name, as it shows in Windows Defender Firewall.
pub const RULE_NAME: &str = "Atlas - your own devices";

/// Where a "no" is remembered, inside the install's state folder, so the
/// setup doesn't ask again every time it's opened.
const DECLINED: &str = "firewall_rule_declined";
/// The program this install added the rule for.
const ADDED: &str = "firewall_rule_added";

/// Tailscale's address ranges: IPv4 CGNAT and its IPv6 ULA prefix.
pub const OWN_NETWORKS: &str = "LocalSubnet,100.64.0.0/10,fd7a:115c:a1e0::/48";

/// The `netsh` arguments that add the rule for `exe`.
pub fn add_args(exe: &Path) -> Vec<String> {
    vec![
        "advfirewall".into(),
        "firewall".into(),
        "add".into(),
        "rule".into(),
        format!("name={RULE_NAME}"),
        "dir=in".into(),
        "action=allow".into(),
        format!("program={}", exe.display()),
        "enable=yes".into(),
        "profile=private,domain".into(),
        "protocol=TCP".into(),
        format!("remoteip={OWN_NETWORKS}"),
        "description=Lets your own phone and computers (and friends on the same wifi) reach Atlas. Friends elsewhere come through Tor, which needs no rule.".into(),
    ]
}

/// The `netsh` arguments that look for the rule (no permission needed).
fn show_args() -> Vec<String> {
    vec!["advfirewall".into(), "firewall".into(), "show".into(), "rule".into(), format!("name={RULE_NAME}"), "verbose".into()]
}

/// Does `netsh`'s answer to `show_args` describe our rule, for this exe?
/// A rule left from an install in another folder doesn't count.
pub fn describes_rule_for(shown: &str, exe: &Path) -> bool {
    describes_rule_for_with(shown, exe, &|n| std::env::var(n).ok())
}

/// `describes_rule_for` with the environment passed in, so a test can give it
/// a variable without changing the whole process's environment.
pub fn describes_rule_for_with(shown: &str, exe: &Path, var: &dyn Fn(&str) -> Option<String>) -> bool {
    let want = plain_path(&exe.display().to_string());
    shown.lines().any(|l| {
        let low = l.to_lowercase();
        if !low.contains("program") {
            return false;
        }
        // The value after the label, with any %VARIABLE% Windows stored in
        // it spelled out (29 Sep 2026: a rule Windows showed as
        // %LOCALAPPDATA%\... never matched, so setup asked for Windows'
        // permission again every time it ran).
        let value = l.split_once(':').map(|(_, v)| v).unwrap_or(l);
        let got = plain_path(&expand_vars_with(value.trim(), var));
        !got.is_empty() && (got == want || low.trim_end().ends_with(&want))
    })
}

/// A path as compared: lower case, `\\?\` dropped, one kind of slash.
fn plain_path(p: &str) -> String {
    p.trim().trim_start_matches(r"\\?\").replace('/', "\\").to_lowercase()
}

/// `%NAME%` in `s` replaced by the environment's value, where there is one.
pub fn expand_vars(s: &str) -> String {
    expand_vars_with(s, &|n| std::env::var(n).ok())
}

/// `expand_vars` with the environment passed in.
pub fn expand_vars_with(s: &str, var: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(a) = rest.find('%') {
        let Some(b) = rest[a + 1..].find('%').map(|b| b + a + 1) else { break };
        let name = &rest[a + 1..b];
        match var(name) {
            Some(v) if !name.is_empty() => {
                out.push_str(&rest[..a]);
                out.push_str(&v);
            }
            _ => out.push_str(&rest[..=b]),
        }
        rest = &rest[b + 1..];
    }
    out.push_str(rest);
    out
}

/// A PowerShell line that runs `netsh` with `args` as administrator: Windows
/// shows its own "allow changes" prompt, and nothing else is elevated.
pub fn elevated_command(args: &[String]) -> String {
    // Each argument single-quoted for PowerShell, with its own quotes doubled;
    // then wrapped in double quotes for netsh, which splits on spaces.
    let list = args
        .iter()
        .map(|a| format!("'\"{}\"'", a.replace('\'', "''").replace('"', "")))
        .collect::<Vec<_>>()
        .join(",");
    format!("$p = Start-Process -FilePath netsh.exe -ArgumentList {list} -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode")
}

/// Where the setup stands on the rule, in words for its step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// Already there for this Atlas.
    Here,
    /// Added just now.
    Added,
    /// You said no before (or just now); the setup won't ask again.
    Declined(String),
    /// Not Windows: nothing to add.
    NotNeeded,
    /// Something went wrong, in words.
    Problem(String),
}

impl Standing {
    pub fn plain(&self) -> String {
        match self {
            Standing::Here => "already there".into(),
            Standing::Added => "added: only Atlas, only your own networks, not on public wifi".into(),
            Standing::Declined(w) => w.clone(),
            Standing::NotNeeded => "nothing to do on this system".into(),
            Standing::Problem(w) => w.clone(),
        }
    }
}

fn declined_words() -> String {
    "not added, as you chose. Windows will ask the first time your phone connects; \
     answer Allow there, or open setup again after deleting data/state/firewall_rule_declined."
        .into()
}

/// Make sure the rule is there, asking once. `run` runs a program and returns
/// (succeeded, what it printed) -- Windows' `netsh` and `powershell`, or a
/// stand-in in a test.
pub fn ensure(state: &Path, exe: &Path, run: &dyn Fn(&str, &[String]) -> (bool, String)) -> Standing {
    if !cfg!(windows) && !cfg!(test) {
        return Standing::NotNeeded;
    }
    let (_, shown) = run("netsh", &show_args());
    if describes_rule_for(&shown, exe) {
        return Standing::Here;
    }
    // Added by this install before, and a rule of that name is still there:
    // not asked for again because netsh words it in a way not read above.
    let added_for = std::fs::read_to_string(state.join(ADDED)).unwrap_or_default();
    if plain_path(&added_for) == plain_path(&exe.display().to_string()) && shown.contains(RULE_NAME) {
        return Standing::Here;
    }
    if state.join(DECLINED).is_file() {
        return Standing::Declined(declined_words());
    }
    let ps = elevated_command(&add_args(exe));
    let (ok, said) = run("powershell", &["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), ps]);
    if ok {
        let (_, shown) = run("netsh", &show_args());
        if describes_rule_for(&shown, exe) || shown.contains(RULE_NAME) {
            crate::heard!(std::fs::create_dir_all(state));
            crate::kept!(std::fs::write(state.join(ADDED), exe.display().to_string()));
            return Standing::Added;
        }
        return Standing::Problem("Windows said yes but the rule isn't there; Windows will ask the first time your phone connects.".into());
    }
    // Only a real "no" is remembered, so the setup isn't a nag. Anything
    // else -- PowerShell missing or blocked by policy, netsh failing -- is a
    // problem to try again next time, not your choice (28 Sep 2026: every
    // failure used to be written down as "you chose no", for good).
    if said_no(&said) {
        crate::heard!(std::fs::create_dir_all(state));
        crate::kept!(std::fs::write(state.join(DECLINED), crate::store::now().to_string()));
        return Standing::Declined(declined_words());
    }
    let first = said.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("it didn't say why");
    Standing::Problem(format!(
        "not added: Windows couldn't make the rule ({first}). I'll try again the next time setup runs; until \
         then Windows will ask the first time your phone connects -- answer Allow."
    ))
}

/// What a test is told instead of a prompt: a No (`said_no` reads it so).
const NOT_IN_A_TEST: &str = "not asked: a test never asks Windows for administrator rights (treated as cancelled by the user)";

/// Would running this put up Windows' "allow changes" prompt?
fn asks_for_elevation(args: &[String]) -> bool {
    args.iter().any(|a| a.to_lowercase().contains("-verb runas"))
}

/// Is this what Windows says when you answer No (or close) the "allow
/// changes" prompt? `Start-Process -Verb RunAs` fails with "The operation
/// was canceled by the user", Windows error 1223 (ERROR_CANCELLED).
pub fn said_no(output: &str) -> bool {
    let o = output.to_lowercase();
    o.contains("canceled by the user") || o.contains("cancelled by the user") || o.contains("1223")
}

/// The real programs, hidden.
pub fn run_program(program: &str, args: &[String]) -> (bool, String) {
    // A test never asks Windows for administrator rights (6 Oct 2026): the
    // setup walk's test ran this for real on the laptop, put up a "make
    // changes" prompt for a firewall rule letting a throwaway atlas.exe in a
    // temp folder take incoming connections, and hung the suite waiting on
    // it. Refused before anything starts, worded as the No it amounts to.
    if asks_for_elevation(args) && crate::roots::under_test() {
        return (false, NOT_IN_A_TEST.to_string());
    }
    let mut cmd = crate::tools::command(program);
    cmd.args(args).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    match cmd.output() {
        Ok(o) => (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
        Err(e) => (false, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_never_puts_up_a_windows_prompt() {
        // The real runner, given exactly what `ensure` sends to add the rule:
        // refused before anything starts, and read as a No.
        let ps = elevated_command(&add_args(&exe()));
        let args = ["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), ps];
        assert!(asks_for_elevation(&args));
        let (ok, said) = run_program("powershell", &args);
        assert!(!ok);
        assert!(said_no(&said), "{said}");
        // Reading the rules asks nothing and is not refused.
        assert!(!asks_for_elevation(&show_args()));
    }
    use std::cell::RefCell;

    fn exe() -> std::path::PathBuf {
        std::path::PathBuf::from(r"C:\Users\sam\Atlas\atlas.exe")
    }

    #[test]
    fn the_rule_is_only_atlas_only_incoming_only_your_own_networks_and_never_public_wifi() {
        let a = add_args(&exe()).join(" ");
        assert!(a.contains(r"program=C:\Users\sam\Atlas\atlas.exe"), "{a}");
        assert!(a.contains("dir=in") && a.contains("action=allow") && a.contains("protocol=TCP"), "{a}");
        assert!(a.contains("profile=private,domain") && !a.contains("public"), "{a}");
        assert!(a.contains("remoteip=LocalSubnet,100.64.0.0/10,fd7a:115c:a1e0::/48"), "{a}");
        assert!(!a.contains("localport=any") && !a.contains("remoteip=any"), "{a}");
    }

    #[test]
    fn the_elevated_line_keeps_each_argument_whole() {
        let ps = elevated_command(&add_args(&exe()));
        assert!(ps.contains("-Verb RunAs") && ps.contains("-Wait"), "{ps}");
        assert!(ps.contains(r#"'"program=C:\Users\sam\Atlas\atlas.exe"'"#), "{ps}");
        assert!(ps.contains(&format!("'\"name={RULE_NAME}\"'")), "{ps}");
    }

    #[test]
    fn a_rule_is_found_only_for_this_atlas() {
        let shown = format!("Rule Name: {RULE_NAME}\nProgram:                              {}\n", exe().display());
        assert!(describes_rule_for(&shown, &exe()));
        let elsewhere = format!("Rule Name: {RULE_NAME}\nProgram: D:\\old\\atlas.exe\n");
        assert!(!describes_rule_for(&elsewhere, &exe()));
        assert!(!describes_rule_for("No rules match the specified criteria.", &exe()));
    }

    #[test]
    fn asked_once_added_then_found_and_a_no_is_remembered() {
        let dir = std::env::temp_dir().join(format!("atlas-doorrule-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // Windows says yes: the rule appears after the elevated add.
        let added = RefCell::new(false);
        let calls = RefCell::new(Vec::new());
        let yes = |p: &str, a: &[String]| {
            calls.borrow_mut().push(p.to_string());
            if p == "powershell" {
                *added.borrow_mut() = true;
                return (true, String::new());
            }
            let shown = if *added.borrow() { format!("Program: {}", exe().display()) } else { "No rules match".into() };
            let _ = a;
            (true, shown)
        };
        assert_eq!(ensure(&dir, &exe(), &yes), Standing::Added);
        assert_eq!(ensure(&dir, &exe(), &yes), Standing::Here, "asked twice");
        assert_eq!(calls.borrow().iter().filter(|c| *c == "powershell").count(), 1);
        // A no (the prompt cancelled) is remembered and not asked again.
        let dir2 = dir.join("second");
        let asked = RefCell::new(0);
        let no = |p: &str, _: &[String]| {
            if p == "powershell" {
                *asked.borrow_mut() += 1;
                return (false, "The operation was canceled by the user.".into());
            }
            (true, "No rules match".into())
        };
        assert!(matches!(ensure(&dir2, &exe(), &no), Standing::Declined(_)));
        assert!(matches!(ensure(&dir2, &exe(), &no), Standing::Declined(_)));
        assert_eq!(*asked.borrow(), 1, "a no was asked again");
        // A failure that isn't a no is said, and tried again next time
        // (28 Sep 2026: it used to be remembered as your no).
        let dir3 = dir.join("third");
        let tried = RefCell::new(0);
        let broken = |p: &str, _: &[String]| {
            if p == "powershell" {
                *tried.borrow_mut() += 1;
                return (false, "powershell : running scripts is disabled on this system".into());
            }
            (true, "No rules match".into())
        };
        assert!(matches!(ensure(&dir3, &exe(), &broken), Standing::Problem(ref w) if w.contains("try again")));
        assert!(matches!(ensure(&dir3, &exe(), &broken), Standing::Problem(_)));
        assert_eq!(*tried.borrow(), 2, "a failure is retried, not taken as a no");
        assert!(said_no("Start-Process : This command cannot be run due to the error: The operation was canceled by the user."));
        assert!(!said_no("netsh failed: access denied"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
