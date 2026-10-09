//! Opening zips, and scanning for viruses before anything is opened (Eric's
//! ruling H3, 25 Sep 2026: "Atlas unzips when needed, and scans for viruses
//! before opening anything — Windows Defender on the file, and on everything
//! a zip unpacks to").
//!
//! The zip reader is Atlas's own: a zip is a list at the end of the file
//! saying where each entry is, and each entry is stored as-is or deflated.
//! No outside program, so nothing to be missing on the day you need it.
//!
//! What it refuses, before unpacking a byte: names that climb out of the
//! folder (`..\..\Windows\…`), absolute paths, archives nested deeper than
//! `files.max_archive_depth`, and anything that claims to grow past
//! `files.max_unpacked_mb` (`files::safe_to_unpack`). What it never does:
//! write over a file that is already there.
//!
//! The scan runs Windows Defender's own command-line scanner with
//! `-DisableRemediation`, so Defender reports what it finds and Atlas tells
//! you, rather than something of yours vanishing into quarantine unsaid. If
//! the scan can't run, nothing is opened until you say so.

use serde::Deserialize;
use std::path::{Path, PathBuf};

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("I can't open {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(|e| format!("I couldn't read {}: {e}", path.display()))?;
    if bytes.len() as u64 > limit { return Err(format!("{} exceeds the current {} MiB reading limit", path.display(), limit / 1024 / 1024)); }
    Ok(bytes)
}

/// One file inside a zip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// Size once unpacked, as the zip claims it.
    pub size: u64,
    pub compressed: u64,
    method: u16,
    local_header: u64,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/') || self.name.ends_with('\\')
    }
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}
fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

/// What's in a zip, from its central directory.
pub fn entries_of(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    // The end-of-directory record is in the last 64 KB + 22 bytes.
    let from = bytes.len().saturating_sub(65_557);
    let eocd = (from..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| bytes[i..].starts_with(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or("that isn't a zip I can read")?;
    let count = u16_at(bytes, eocd + 10).ok_or("the zip is cut short")? as usize;
    let dir_at = u32_at(bytes, eocd + 16).ok_or("the zip is cut short")? as usize;
    if dir_at == 0xFFFF_FFFF {
        return Err("it's a very large (zip64) archive, which I can't open yet".into());
    }
    let mut out = Vec::with_capacity(count);
    let mut i = dir_at;
    for _ in 0..count {
        if !bytes.get(i..).is_some_and(|b| b.starts_with(&[0x50, 0x4b, 0x01, 0x02])) {
            return Err("the zip's list of contents is damaged".into());
        }
        let flags = u16_at(bytes, i + 8).unwrap_or(0);
        if flags & 1 == 1 {
            return Err("it's password-protected, so I can't open it".into());
        }
        let method = u16_at(bytes, i + 10).unwrap_or(0);
        let compressed = u32_at(bytes, i + 20).unwrap_or(0) as u64;
        let size = u32_at(bytes, i + 24).unwrap_or(0) as u64;
        let name_len = u16_at(bytes, i + 28).unwrap_or(0) as usize;
        let extra_len = u16_at(bytes, i + 30).unwrap_or(0) as usize;
        let comment_len = u16_at(bytes, i + 32).unwrap_or(0) as usize;
        let local = u32_at(bytes, i + 42).unwrap_or(0) as u64;
        let name_bytes = bytes.get(i + 46..i + 46 + name_len).ok_or("the zip is cut short")?;
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        out.push(Entry { name, size, compressed, method, local_header: local });
        i += 46 + name_len + extra_len + comment_len;
    }
    Ok(out)
}

/// The bytes of one entry, unpacked.
pub fn read_entry(bytes: &[u8], e: &Entry) -> Result<Vec<u8>, String> {
    let at = e.local_header as usize;
    if !bytes.get(at..).is_some_and(|b| b.starts_with(&[0x50, 0x4b, 0x03, 0x04])) {
        return Err(format!("{} is damaged in the zip", e.name));
    }
    let name_len = u16_at(bytes, at + 26).unwrap_or(0) as usize;
    let extra_len = u16_at(bytes, at + 28).unwrap_or(0) as usize;
    let start = at + 30 + name_len + extra_len;
    let data = bytes.get(start..start + e.compressed as usize).ok_or_else(|| format!("{} is cut short", e.name))?;
    match e.method {
        0 => Ok(data.to_vec()),
        8 => miniz_oxide::inflate::decompress_to_vec_with_limit(data, e.size as usize + 1)
            .map_err(|_| format!("{} didn't unpack cleanly", e.name)),
        m => Err(format!("{} is packed a way I can't unpack (method {m})", e.name)),
    }
}

/// A name that stays inside the folder it's unpacked into, or why not.
pub fn name_inside(name: &str) -> Result<PathBuf, String> {
    let n = name.replace('\\', "/");
    if n.starts_with('/') || n.chars().nth(1) == Some(':') {
        return Err(format!("{name} points outside the folder"));
    }
    let mut p = PathBuf::new();
    for part in n.split('/') {
        match part {
            "" | "." => continue,
            ".." => return Err(format!("{name} tries to climb out of the folder")),
            // A colon anywhere is a drive (`a/C:x` replaces the whole path when
            // pushed on Windows) or an NTFS alternate stream (`x:hidden`);
            // neither is a file in this folder (Q8).
            s if s.contains(':') => return Err(format!("{name} points outside the folder")),
            s if windows_reserved(s) => return Err(format!("{name} is a name Windows keeps for devices")),
            s => p.push(s),
        }
    }
    if p.as_os_str().is_empty() {
        return Err("an entry with no name".into());
    }
    Ok(p)
}

/// CON, PRN, AUX, NUL, COM1-9 and LPT1-9, with or without an extension or
/// trailing dots/spaces: Windows opens the device, not a file.
fn windows_reserved(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").trim_end_matches([' ', '.']).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit() && stem.as_bytes()[3] != b'0')
}

/// How deep the nesting goes: 1 for a plain zip, 2 when it holds zips.
pub fn nesting(entries: &[Entry]) -> u32 {
    if entries.iter().any(|e| e.name.to_lowercase().ends_with(".zip")) {
        2
    } else {
        1
    }
}

/// A folder beside the zip, named for it, that doesn't exist yet.
pub fn folder_beside(zip: &Path) -> PathBuf {
    let parent = zip.parent().unwrap_or_else(|| Path::new("."));
    let stem = zip.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "unzipped".into());
    let mut k = 1;
    loop {
        let name = if k == 1 { stem.clone() } else { format!("{stem} ({k})") };
        let p = parent.join(name);
        if !p.exists() {
            return p;
        }
        k += 1;
    }
}


pub fn unzip_stoppable(zip: &Path, dest: &Path, cfg: &crate::files::FilesConfig, stop: &dyn Fn() -> bool) -> Result<Vec<PathBuf>, String> {
    if stop() { return Err("Stopped before opening the archive".into()); }
    let size = std::fs::metadata(zip).map_err(|e| format!("I can't inspect {}: {e}", zip.display()))?.len();
    if size > 256 * 1024 * 1024 { return Err("this archive exceeds the current 256 MiB preparation limit; use a streaming archive tool for it".into()); }
    let bytes = read_bounded(zip, 256 * 1024 * 1024)?;
    let entries = entries_of(&bytes)?;
    let total = entries.iter().try_fold(0u64, |sum, e| sum.checked_add(e.size)).ok_or("the archive's claimed size is too large")?;
    crate::files::safe_to_unpack((bytes.len() as u64).div_ceil(1_000_000), total.div_ceil(1_000_000), nesting(&entries), cfg)?;
    let names: Vec<PathBuf> = entries.iter().map(|e| name_inside(&e.name)).collect::<Result<_, _>>()?;
    if dest.exists() {
        return Err(format!("{} is already there, and I don't unpack over things", dest.display()));
    }
    if stop() { return Err("Stopped before creating the unpacked folder".into()); }
    std::fs::create_dir(dest).map_err(|e| format!("I couldn't make {}: {e}", dest.display()))?;
    let mut written = Vec::new();
    for (e, rel) in entries.iter().zip(names) {
        if stop() { return Err(format!("Stopped after writing {} files", written.len())); }
        let to = dest.join(&rel);
        // A previously extracted directory must never redirect subsequent
        // entries through a link outside the destination.
        let mut parent = to.parent();
        while let Some(p) = parent {
            if let Ok(meta) = std::fs::symlink_metadata(p) {
                if meta.file_type().is_symlink() { return Err(format!("{} contains a linked folder; unpacking stopped", rel.display())); }
            }
            if p == dest { break; }
            parent = p.parent();
        }
        if e.is_dir() {
            std::fs::create_dir_all(&to).map_err(|err| format!("I couldn't make {}: {err}", rel.display()))?;
            continue;
        }
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p).map_err(|err| format!("I couldn't make {}: {err}", p.display()))?;
        }
        let data = read_entry(&bytes, e)?;
        if data.len() as u64 != e.size { return Err(format!("{} has a different size than the archive claims", e.name)); }
        if stop() { return Err(format!("Stopped after writing {} files", written.len())); }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&to).map_err(|err| format!("I couldn't create {} without overwriting: {err}", rel.display()))?;
        file.write_all(&data).and_then(|()| file.sync_all()).map_err(|err| format!("I couldn't save {}: {err}", rel.display()))?;
        written.push(to);
    }
    Ok(written)
}

/// The words of a Word document (.docx is a zip of XML).
pub fn docx_text(path: &Path) -> Result<String, String> {
    if std::fs::metadata(path).map_err(|e| format!("I can't inspect it: {e}"))?.len() > 64 * 1024 * 1024 {
        return Err("this Word document exceeds the current 64 MiB reading limit".into());
    }
    let bytes = read_bounded(path, 64 * 1024 * 1024)?;
    let entries = entries_of(&bytes)?;
    let doc = entries.iter().find(|e| e.name == "word/document.xml").ok_or("that isn't a Word document inside")?;
    if doc.size > 32 * 1024 * 1024 { return Err("the text inside this Word document exceeds the current 32 MiB reading limit".into()); }
    let xml = String::from_utf8_lossy(&read_entry(&bytes, doc)?).into_owned();
    let mut out = String::new();
    let mut rest = xml.as_str();
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let Some(gt) = rest[lt..].find('>') else { break };
        let tag = &rest[lt + 1..lt + gt];
        if tag.starts_with("/w:p") && !tag.starts_with("/w:pPr") {
            out.push('\n');
        } else if tag.starts_with("w:tab") || tag.starts_with("w:br") {
            out.push(' ');
        }
        rest = &rest[lt + gt + 1..];
    }
    Ok(out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").trim().to_string())
}

// ------------------------------------------------------------------ scanning

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ScanConfig {
    /// The scanner. `{ProgramFiles}` is filled from the environment.
    pub command: String,
    /// `{path}` is the file or folder being scanned.
    pub args: Vec<String>,
    /// The exit code that means "found something". Defender uses 2.
    pub threat_exit: i32,
}

impl Default for ScanConfig {
    fn default() -> Self {
        if cfg!(windows) {
            ScanConfig {
                command: "{ProgramFiles}\\Windows Defender\\MpCmdRun.exe".into(),
                args: ["-Scan", "-ScanType", "3", "-File", "{path}", "-DisableRemediation"].map(String::from).to_vec(),
                threat_exit: 2,
            }
        } else {
            ScanConfig { command: String::new(), args: vec![], threat_exit: 2 }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Clean,
    /// What the scanner named.
    Threat(String),
    /// Why it couldn't scan. Nothing is opened on this.
    NotScanned(String),
}

impl Verdict {
    /// For the end of a sentence about the file.
    pub fn said(&self) -> String {
        match self {
            Verdict::Clean => "Windows Defender found nothing in it".into(),
            Verdict::Threat(t) => format!(
                "Windows Defender found {t} in it, so I haven't opened it. I've left it where it is — \
                 Defender's own screen can quarantine it"
            ),
            Verdict::NotScanned(why) => format!("I couldn't scan it for viruses ({why}), so I haven't opened it"),
        }
    }
}


pub fn scan_stoppable(path: &Path, cfg: &ScanConfig, stop: &dyn Fn() -> bool) -> Verdict {
    if stop() { return Verdict::NotScanned("the scan was stopped before starting".into()); }
    if cfg.command.trim().is_empty() {
        return Verdict::NotScanned("there's no virus scanner set up on this machine".into());
    }
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    let cmd = cfg.command.replace("{ProgramFiles}", &pf);
    let args: Vec<String> = cfg.args.iter().map(|a| a.replace("{path}", &path.display().to_string())).collect();
    let mut c = crate::tools::command(&cmd);
    c.args(&args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // no console window
    }
    let out = crate::tools::run_scoped(&mut c, std::time::Duration::from_secs(180), 2 * 1024 * 1024, None, Some(stop));
    let status = match &out.end {
        crate::tools::ProcessEnd::Exited(status) if !out.truncated => *status,
        crate::tools::ProcessEnd::Exited(_) => return Verdict::NotScanned("the scanner's output exceeded its capture budget; the result is unconfirmed".into()),
        crate::tools::ProcessEnd::Stopped => return Verdict::NotScanned("the scan was stopped".into()),
        crate::tools::ProcessEnd::TimedOut => return Verdict::NotScanned("the scan did not finish within 180 seconds".into()),
        crate::tools::ProcessEnd::Failed(why) => return Verdict::NotScanned(format!("the scan did not finish: {why}")),
    };
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    match status.code() {
        Some(0) => Verdict::Clean,
        Some(code) if code == cfg.threat_exit => Verdict::Threat(threat_named(&text)),
        Some(code) => Verdict::NotScanned(format!("the scanner stopped with code {code}")),
        None => Verdict::NotScanned("the scanner was stopped".into()),
    }
}

/// Defender prints `Threat                  : Virus:DOS/EICAR_Test_File`.
fn threat_named(text: &str) -> String {
    text.lines()
        .find_map(|l| {
            let t = l.trim();
            t.strip_prefix("Threat").and_then(|r| r.trim_start().strip_prefix(':')).map(|n| n.trim().to_string())
        })
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "a threat".into())
}
