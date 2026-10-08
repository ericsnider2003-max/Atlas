//! Organizing a folder: a plan said first, carried out on one yes, every
//! move taken back by "undo that".
//!
//! Eric, 2 Oct 2026: "can't organize my PC properly." What there was before
//! this: "organize my PC" looked at the loose files on the desktop and in
//! Downloads only, filed the ones whose extension it recognised into PARA
//! folders under Documents\Filed (`filing::suggest` -- a .pdf to Resources,
//! a .docx to Projects, a year untouched to Archive), left every picture,
//! video, archive and installer where it was, found no duplicates, and wrote
//! nothing where "undo" could find it. You couldn't name a folder, and
//! "sort the files in ..." or "find duplicates in ..." reached nothing.
//!
//! What this does, in order:
//!
//! - **Which folder.** The one you name -- a full path, or "my desktop",
//!   "my downloads", "my documents", "my pictures" -- or, for "organize my
//!   PC", the desktop, Downloads and Documents. Each found the way this
//!   machine has it (`user_folder`): on Windows asked of Windows itself,
//!   because Downloads can live on another drive and OneDrive moves the
//!   desktop; on Linux the XDG user folders; otherwise under home. Nothing
//!   here names anyone's folders.
//! - **The plan.** The loose files at the top of that folder, grouped by
//!   kind (documents, images, video, audio, archives, installers, code,
//!   spreadsheets, presentations, ebooks) into a folder for each, inside the
//!   same folder or the one you said to put them in. Untouched for a year,
//!   into a year folder inside its kind. Copies of the same file (same size,
//!   then the same hash, then the same bytes) and installers more than a
//!   month old go to "To review", which you can look through and empty
//!   yourself. Empty folders are counted and left. A kind it can't place is
//!   left where you put it, and said.
//! - **Never.** Never deletes. Never writes over a file (a name already
//!   taken gets " (2)"). Never touches hidden or system files, shortcuts,
//!   a download still arriving, or a file open in another program. Never
//!   sorts a system folder, a program's folder or a code project, where
//!   moving one file breaks the rest. At most `MOST_AT_ONCE` files a time.
//! - **Permission.** The same switch and the same gate as everything else
//!   that moves your files: System changes on, and every move through
//!   `system::judge`, which keeps to the folders Atlas may work in.
//! - **Undo.** Every move is kept beside its line in the history
//!   (`tune::TuneUndo::Organized`), so "undo that" puts each file back where
//!   it was and takes away the folders this made, if they're empty again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The most files moved in one go. A bigger folder is done in turns: the
/// plan says how many are left for next time.
pub const MOST_AT_ONCE: usize = 500;

/// Where copies and old installers go, inside the folder being sorted.
pub const TO_REVIEW: &str = "To review";

/// An installer this many days old has done its job.
const OLD_INSTALLER_DAYS: u64 = 30;

/// Untouched this long, it goes in a year folder inside its kind.
const OLD_FILE_DAYS: u64 = 365;

/// Changed this recently, it may still be downloading or being saved.
const STILL_ARRIVING_SECS: u64 = 120;

/// How deep the look for copies and empty folders goes.
const DEEPEST: usize = 4;

/// What a file is, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileKind {
    Documents,
    Spreadsheets,
    Presentations,
    Ebooks,
    Images,
    Video,
    Audio,
    Archives,
    Installers,
    Code,
}

impl FileKind {
    /// The folder each kind goes into.
    pub fn folder(&self) -> &'static str {
        match self {
            FileKind::Documents => "Documents",
            FileKind::Spreadsheets => "Spreadsheets",
            FileKind::Presentations => "Presentations",
            FileKind::Ebooks => "Ebooks",
            FileKind::Images => "Pictures",
            FileKind::Video => "Videos",
            FileKind::Audio => "Music",
            FileKind::Archives => "Archives",
            FileKind::Installers => "Installers",
            FileKind::Code => "Code",
        }
    }

    fn plural(&self) -> &'static str {
        match self {
            FileKind::Documents => "documents",
            FileKind::Spreadsheets => "spreadsheets",
            FileKind::Presentations => "presentations",
            FileKind::Ebooks => "ebooks",
            FileKind::Images => "pictures",
            FileKind::Video => "videos",
            FileKind::Audio => "audio files",
            FileKind::Archives => "archives",
            FileKind::Installers => "installers",
            FileKind::Code => "code files",
        }
    }
}

/// The kind of file an extension says it is, or None for one Atlas has no
/// confident home for (it's left where it is, like `filing` leaves it).
fn kind_for(ext: &str) -> Option<FileKind> {
    let e = ext.to_ascii_lowercase();
    let k = match e.as_str() {
        "pdf" | "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "pages" | "wpd" | "tex" | "xps" | "oxps" => FileKind::Documents,
        "xls" | "xlsx" | "xlsm" | "ods" | "csv" | "tsv" | "numbers" => FileKind::Spreadsheets,
        "ppt" | "pptx" | "odp" | "key" => FileKind::Presentations,
        "epub" | "mobi" | "azw" | "azw3" | "djvu" | "fb2" | "cbz" | "cbr" => FileKind::Ebooks,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tif" | "tiff" | "webp" | "heic" | "heif" | "svg" | "ico" | "raw" | "cr2"
        | "nef" | "arw" | "dng" | "psd" | "xcf" | "avif" => FileKind::Images,
        "mp4" | "mkv" | "mov" | "avi" | "wmv" | "webm" | "flv" | "m4v" | "mpg" | "mpeg" | "3gp" => FileKind::Video,
        "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" | "opus" | "wma" | "aiff" | "mid" | "midi" => FileKind::Audio,
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "cab" => FileKind::Archives,
        "exe" | "msi" | "msix" | "msixbundle" | "appx" | "appxbundle" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" | "iso" => {
            FileKind::Installers
        }
        "py" | "rs" | "js" | "jsx" | "tsx" | "c" | "h" | "cpp" | "hpp" | "cs" | "java" | "go" | "rb" | "php" | "html"
        | "css" | "json" | "yaml" | "yml" | "toml" | "xml" | "sql" | "sh" | "ps1" | "bat" | "ipynb" | "kt" | "swift" | "lua" => {
            FileKind::Code
        }
        _ => return None,
    };
    Some(k)
}

/// Files a folder like the desktop holds that aren't loose files: the
/// shortcuts you launch things from and the folder's own settings.
const NOT_LOOSE: &[&str] = &["lnk", "url", "ini", "desktop", "website", "appref-ms", "library-ms", "sys", "dll", "lock"];

/// A download that hasn't finished.
const STILL_DOWNLOADING: &[&str] = &["crdownload", "part", "partial", "download", "opdownload", "tmp", "!ut", "aria2"];

/// Folders inside which moving one file breaks the rest: a code project, a
/// program's own folder, a library. Their presence makes a folder off limits.
const PROJECT_MARKS: &[&str] = &[
    ".git", ".hg", ".svn", "cargo.toml", "package.json", "pyproject.toml", "setup.py", "go.mod", "pom.xml", "build.gradle",
    "makefile", "cmakelists.txt", ".vscode", ".idea", "unins000.exe", "uninstall.exe",
];

/// Folders never sorted or walked into, wherever they are.
const NEVER_INSIDE: &[&str] = &[
    "windows", "program files", "program files (x86)", "programdata", "appdata", "$recycle.bin", "system volume information",
    "node_modules", "library", "applications", "system", "recovery", "$windows.~bt", "msocache", "perflogs",
];

/// Why a file is in the plan.
#[derive(Debug, Clone, PartialEq)]
pub enum Reason {
    /// Into its kind's folder (and a year folder when old).
    Kind { kind: FileKind, year: Option<i64> },
    /// The same bytes as another file, which stays.
    Copy { of: PathBuf },
    /// An installer more than a month old.
    OldInstaller,
}

/// One move the plan would make.
#[derive(Debug, Clone, PartialEq)]
pub struct SortMove {
    pub from: PathBuf,
    /// The folder it goes into. The name is kept, or given " (2)" there.
    pub into: PathBuf,
    pub reason: Reason,
}

/// What sorting a folder would do, before anything is done.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SortPlan {
    /// The folders looked at.
    pub folders: Vec<PathBuf>,
    pub moves: Vec<SortMove>,
    /// Loose files with no confident home, left where they are.
    pub unplaced: Vec<PathBuf>,
    /// Hidden or system files, shortcuts, downloads still arriving -- not
    /// counted as yours to sort.
    pub passed_over: usize,
    /// Open in another program right now.
    pub in_use: Vec<PathBuf>,
    /// Folders with nothing in them. Said, never removed.
    pub empty_folders: Vec<PathBuf>,
    /// Folders refused, with why (a system folder, a code project ...).
    pub refused: Vec<(PathBuf, String)>,
    /// Moves beyond `MOST_AT_ONCE`, left for the next turn.
    pub more_next_time: usize,
    /// False when the time ran out before every file was compared.
    pub complete: bool,
    /// "Find duplicates in ...": only the copies move.
    pub copies_only: bool,
}

impl SortPlan {
    pub fn has_moves(&self) -> bool {
        !self.moves.is_empty()
    }
}

/// What was asked, read from the whole sentence.
#[derive(Debug, Clone, PartialEq)]
pub struct SortRequest {
    /// The folders to sort, found on this machine.
    pub folders: Vec<PathBuf>,
    /// A folder named after "into" / "to", where the sorted folders go.
    pub into: Option<PathBuf>,
    /// "Find duplicates in ...".
    pub copies_only: bool,
    /// A folder was named that couldn't be found: its words, to ask about.
    pub not_found: Option<String>,
}

/// The folders people mean by name, as this machine has them.
///
/// Windows: asked of Windows (`SHGetKnownFolderPath`), so a Downloads moved
/// to D:\ or a desktop OneDrive backs up is the one found. Linux: the XDG
/// user folders (`~/.config/user-dirs.dirs`), which are named in the
/// person's own language. Otherwise, and when neither says, the folder of
/// that name under home (and under home\OneDrive, where Windows puts it when
/// OneDrive backs it up). `which` is "desktop", "downloads", "documents",
/// "pictures", "videos" or "music".
pub fn user_folder(which: &str) -> Option<PathBuf> {
    let which = which.to_ascii_lowercase();
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::{
            FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos,
        };
        let id = match which.as_str() {
            "desktop" => Some(&FOLDERID_Desktop),
            "downloads" => Some(&FOLDERID_Downloads),
            "documents" => Some(&FOLDERID_Documents),
            "pictures" => Some(&FOLDERID_Pictures),
            "videos" => Some(&FOLDERID_Videos),
            "music" => Some(&FOLDERID_Music),
            _ => None,
        };
        if let Some(p) = id.and_then(windows_known_folder) {
            return Some(p);
        }
    }
    let home = PathBuf::from(crate::doctor::lookup_env("USERPROFILE").or_else(|| crate::doctor::lookup_env("HOME"))?);
    #[cfg(not(windows))]
    {
        let var = match which.as_str() {
            "desktop" => "XDG_DESKTOP_DIR",
            "downloads" => "XDG_DOWNLOAD_DIR",
            "documents" => "XDG_DOCUMENTS_DIR",
            "pictures" => "XDG_PICTURES_DIR",
            "videos" => "XDG_VIDEOS_DIR",
            "music" => "XDG_MUSIC_DIR",
            _ => "",
        };
        let config = crate::doctor::lookup_env("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
        if let Ok(text) = std::fs::read_to_string(config.join("user-dirs.dirs")) {
            if let Some(p) = xdg_folder(&text, var, &home) {
                // A folder named home itself means "not set" in that file.
                if p != home {
                    return Some(p);
                }
            }
        }
    }
    let name = match which.as_str() {
        "desktop" => "Desktop",
        "downloads" => "Downloads",
        "documents" => "Documents",
        "pictures" => "Pictures",
        "videos" => "Videos",
        "music" => "Music",
        _ => return None,
    };
    [home.join("OneDrive").join(name), home.join(name)].into_iter().find(|d| d.is_dir()).or_else(|| Some(home.join(name)))
}

#[cfg(windows)]
fn windows_known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    crate::firstlaunch::known_folder(id)
}

/// One line of `user-dirs.dirs`: `XDG_DOWNLOAD_DIR="$HOME/Downloads"`.
#[cfg(not(windows))]
fn xdg_folder(text: &str, var: &str, home: &Path) -> Option<PathBuf> {
    if var.is_empty() {
        return None;
    }
    let line = text.lines().map(str::trim).find(|l| l.starts_with(var) && l[var.len()..].trim_start().starts_with('='))?;
    let v = line.split_once('=')?.1.trim().trim_matches('"');
    let v = v.replace("$HOME", &home.display().to_string());
    (!v.is_empty()).then(|| PathBuf::from(v))
}

/// A path written out in full: "/home/x/Stuff", "D:\Photos". "D:\..." is
/// read as a path on every system, so a test reads it the same anywhere.
fn full_path(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim().trim_end_matches(['.', '?', '!', ',']).trim().trim_matches(['"', '\'']).trim();
    if raw.is_empty() {
        return None;
    }
    let b = raw.as_bytes();
    let windows_abs = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    let p = PathBuf::from(raw);
    (p.is_absolute() || windows_abs).then_some(p)
}

/// The folder named by `words` -- a full path, one of the folders people
/// mean by name ("my downloads", "the desktop"), or the name of a folder
/// directly inside home, the desktop or Documents that is there.
fn named_folder(words: &str) -> Option<PathBuf> {
    if let Some(p) = full_path(words) {
        return Some(p);
    }
    let w = words.trim().trim_end_matches(['.', '?', '!', ',']).to_lowercase();
    let w = w.trim_start_matches("my ").trim_start_matches("the ").trim();
    let w = w.trim_end_matches(" folder").trim_end_matches(" directory").trim();
    for (said, which) in [
        ("desktop", "desktop"),
        ("download", "downloads"),
        ("downloads", "downloads"),
        ("documents", "documents"),
        ("document", "documents"),
        ("docs", "documents"),
        ("pictures", "pictures"),
        ("photos", "pictures"),
        ("videos", "videos"),
        ("music", "music"),
    ] {
        if w == said {
            return user_folder(which);
        }
    }
    if w.is_empty() || w.contains(['/', '\\']) {
        return None;
    }
    let home = crate::doctor::lookup_env("USERPROFILE").or_else(|| crate::doctor::lookup_env("HOME")).map(PathBuf::from);
    let mut places: Vec<PathBuf> = home.into_iter().collect();
    places.extend(["desktop", "documents"].iter().filter_map(|n| user_folder(n)));
    let found: Vec<PathBuf> = places
        .iter()
        .filter_map(|p| std::fs::read_dir(p).ok())
        .flat_map(|rd| rd.flatten())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| e.file_name().to_string_lossy().to_lowercase() == w)
        .map(|e| e.path())
        .collect();
    // Only one folder by that name: two is a question, not a guess.
    (found.len() == 1).then(|| found[0].clone())
}

/// Read what was said: which folders, where to, and whether only copies.
///
/// "organize my PC" -> the desktop, Downloads and Documents. "clean up my
/// desktop" -> the desktop. "sort the files in D:\Stuff into D:\Sorted" ->
/// that folder, into that one. "find duplicates in my downloads" -> only the
/// copies, from Downloads.
pub fn read_sort_request(said: &str) -> SortRequest {
    // ASCII lowercase only, so a position found in it is the same position
    // in what was said (a full lowercase can change a word's length).
    let lower = said.to_ascii_lowercase();
    let copies_only = ["duplicate", "copies", "same file"].iter().any(|w| lower.contains(w));
    // "into X" / "to X" after the folder: where the sorted folders go.
    let mut rest = said.to_string();
    let mut into = None;
    for w in [" into ", " to "] {
        if let Some(i) = rest.to_ascii_lowercase().rfind(w) {
            if let Some(p) = full_path(&rest[i + w.len()..]) {
                into = Some(p);
                rest.truncate(i);
                break;
            }
        }
    }
    let rl = rest.to_ascii_lowercase();
    // "... in X", "... inside X", "... from X", "... on X".
    let at = [" in ", " inside ", " from ", " on "].iter().filter_map(|w| rl.find(w).map(|i| (i, i + w.len()))).min();
    let before = at.map(|(i, _)| rl[..i].to_string()).unwrap_or_default();
    let named = at.map(|(_, a)| rest[a..].trim().to_string()).filter(|s| !s.is_empty());
    let mut folders = Vec::new();
    let mut not_found = None;
    let everyday = ["desktop", "download", "documents", "pictures", "photos"];
    match named.as_deref().map(|w| (w, named_folder(w))) {
        Some((_, Some(p))) => folders.push(p),
        // "in" that doesn't name a folder ("organize my desktop in a
        // sensible way") when an everyday folder was named before it.
        Some((words, None)) if !everyday.iter().any(|e| before.contains(e)) => {
            not_found = Some(words.to_string())
        }
        _ => {
            // A folder named without "in": "organize my downloads",
            // "clean up my desktop". Several may be named at once.
            for (word, which) in [("desktop", "desktop"), ("download", "downloads"), ("documents", "documents"), ("pictures", "pictures"), ("photos", "pictures")] {
                if lower.contains(word) {
                    if let Some(p) = user_folder(which) {
                        if !folders.contains(&p) {
                            folders.push(p);
                        }
                    }
                }
            }
            if folders.is_empty() {
                // "Organize my PC": the three places loose files pile up.
                for which in ["desktop", "downloads", "documents"] {
                    if let Some(p) = user_folder(which) {
                        if p.is_dir() && !folders.contains(&p) {
                            folders.push(p);
                        }
                    }
                }
            }
        }
    }
    SortRequest { folders, into, copies_only, not_found }
}

/// Why a folder is not one to sort, or None when it is: a system folder,
/// a program's, a code project, a drive's top or the home folder itself
/// (which holds settings programs rely on).
fn refuse_to_sort(dir: &Path) -> Option<String> {
    if !dir.is_dir() {
        return Some(format!("{} isn't a folder I can find", dir.display()));
    }
    let s = dir.display().to_string().replace('\\', "/").to_lowercase();
    let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() <= 1 {
        return Some(format!("{} is the top of a drive -- name a folder in it", dir.display()));
    }
    if parts.iter().any(|p| NEVER_INSIDE.contains(p) || p.starts_with('.')) {
        return Some(format!("{} is where the system or a program keeps its own files", dir.display()));
    }
    let unix_system = ["etc", "usr", "bin", "sbin", "lib", "lib64", "boot", "proc", "sys", "dev", "var", "opt", "root", "snap"];
    if parts.first().is_some_and(|p| unix_system.contains(p)) {
        return Some(format!("{} is where the system keeps its own files", dir.display()));
    }
    let home = crate::doctor::lookup_env("USERPROFILE").or_else(|| crate::doctor::lookup_env("HOME"));
    if home.is_some_and(|h| h.replace('\\', "/").to_lowercase().trim_end_matches('/') == s.trim_end_matches('/')) {
        return Some("your home folder itself holds settings your programs rely on -- name one of the folders in it".into());
    }
    if let Some(mark) = project_mark(dir) {
        return Some(format!("{} looks like a project or a program's folder ({mark} is in it), where moving one file breaks the rest", dir.display()));
    }
    None
}

fn project_mark(dir: &Path) -> Option<String> {
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().to_lowercase();
        if PROJECT_MARKS.contains(&n.as_str()) || n.ends_with(".sln") || n.ends_with(".csproj") {
            return Some(e.file_name().to_string_lossy().to_string());
        }
    }
    None
}

/// Hidden or system, by the file system's own mark (Windows) or a leading
/// dot (everywhere).
fn hidden(path: &Path, meta: &std::fs::Metadata) -> bool {
    if path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if meta.file_attributes() & (HIDDEN | SYSTEM) != 0 {
            return true;
        }
    }
    let _ = meta;
    false
}

/// Open in another program right now. On Windows: a file another program
/// holds can't be opened for ourselves alone. Elsewhere files aren't locked
/// that way, so a file changed in the last two minutes is treated as in use.
fn open_elsewhere(path: &Path, modified: u64, now: u64) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        if std::fs::OpenOptions::new().read(true).share_mode(0).open(path).is_err() {
            return true;
        }
    }
    let _ = path;
    now.saturating_sub(modified) < STILL_ARRIVING_SECS
}

fn secs(t: std::io::Result<std::time::SystemTime>) -> u64 {
    t.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

/// An installer by its name, for a program file (.exe) outside Downloads:
/// a portable program on the desktop is something you run from there.
fn looks_like_installer(name: &str) -> bool {
    let n = name.to_lowercase();
    ["setup", "install", "installer", "x64", "x86", "win64", "win32", "amd64", "update", "redist"].iter().any(|w| n.contains(w))
}

/// One file found while looking.
struct Seen {
    path: PathBuf,
    len: u64,
    modified: u64,
    top: bool,
}

/// The same bytes: compared in pieces, stopping at the first difference.
fn same_bytes(a: &Path, b: &Path) -> bool {
    use std::io::Read;
    let (Ok(mut fa), Ok(mut fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else { return false };
    let mut ba = vec![0u8; 1 << 16];
    let mut bb = vec![0u8; 1 << 16];
    loop {
        let Ok(na) = fa.read(&mut ba) else { return false };
        if na == 0 {
            return fb.read(&mut bb[..1]).map(|n| n == 0).unwrap_or(false);
        }
        if fb.read_exact(&mut bb[..na]).is_err() || ba[..na] != bb[..na] {
            return false;
        }
    }
}

fn content_hash(path: &Path) -> Option<u64> {
    use std::hash::Hasher;
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.write(&buf[..n]);
    }
    Some(h.finish())
}

/// Copies among `files`: grouped by size, then by a hash of the whole
/// file, then checked byte for byte against the one that stays. The oldest
/// of each set stays (the shorter path when they're as old). Each copy with
/// the one it copies. Stops at `until`, saying so.
fn find_copies(files: &[Seen], until: std::time::Instant) -> (Vec<(usize, usize)>, bool) {
    let mut by_size: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (i, f) in files.iter().enumerate() {
        if f.len > 0 {
            by_size.entry(f.len).or_default().push(i);
        }
    }
    let mut out = Vec::new();
    for (_, idx) in by_size.into_iter().rev().filter(|(_, v)| v.len() > 1) {
        if std::time::Instant::now() > until {
            return (out, false);
        }
        let mut by_hash: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
        for i in idx {
            if let Some(h) = content_hash(&files[i].path) {
                by_hash.entry(h).or_default().push(i);
            }
        }
        for (_, mut same) in by_hash.into_iter().filter(|(_, v)| v.len() > 1) {
            same.sort_by(|a, b| {
                files[*a]
                    .modified
                    .cmp(&files[*b].modified)
                    .then(files[*a].path.as_os_str().len().cmp(&files[*b].path.as_os_str().len()))
                    .then(files[*a].path.cmp(&files[*b].path))
            });
            let keep = same[0];
            for c in &same[1..] {
                if same_bytes(&files[keep].path, &files[*c].path) {
                    out.push((*c, keep));
                }
            }
        }
    }
    (out, true)
}

/// Whether `kind`'s folder is the folder being sorted itself -- documents
/// in Documents, pictures in Pictures -- where they're already home.
fn already_home(root: &Path, kind: FileKind) -> bool {
    let name = root.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let aliases: &[&str] = match kind {
        FileKind::Images => &["pictures", "photos", "images"],
        FileKind::Video => &["videos", "movies", "video"],
        FileKind::Audio => &["music", "audio"],
        _ => &[],
    };
    name == kind.folder().to_lowercase() || aliases.contains(&name.as_str())
}

/// The plan for one folder, added to `plan`.
fn plan_one(dir: &Path, into: Option<&Path>, now: u64, until: std::time::Instant, plan: &mut SortPlan) {
    let dest_root = into.map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf());
    let review = dest_root.join(TO_REVIEW);
    let is_downloads = user_folder("downloads").is_some_and(|d| d == dir)
        || dir.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains("download"));
    // Everything in it, a few folders deep, for copies and empty folders;
    // the loose files at the top are the ones sorted.
    let mut seen: Vec<Seen> = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        if std::time::Instant::now() > until {
            plan.complete = false;
            break;
        }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut any = false;
        for e in rd.flatten() {
            any = true;
            let path = e.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            if meta.file_type().is_symlink() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if meta.is_dir() {
                let lower = name.to_lowercase();
                // Not into hidden, system or program folders, nor the
                // "To review" folder this made before, nor a code project.
                if depth + 1 < DEEPEST
                    && !hidden(&path, &meta)
                    && !NEVER_INSIDE.contains(&lower.as_str())
                    && lower != TO_REVIEW.to_lowercase()
                    && project_mark(&path).is_none()
                {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let ext = path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            let top = depth == 0;
            if hidden(&path, &meta) || NOT_LOOSE.contains(&ext.as_str()) || STILL_DOWNLOADING.contains(&ext.as_str()) {
                if top {
                    plan.passed_over += 1;
                }
                continue;
            }
            seen.push(Seen { path, len: meta.len(), modified: secs(meta.modified()), top });
        }
        if !any && depth > 0 {
            plan.empty_folders.push(d);
        }
    }
    seen.sort_by(|a, b| a.path.cmp(&b.path));
    let (copies, all_compared) = find_copies(&seen, until);
    if !all_compared {
        plan.complete = false;
    }
    let copy_of: BTreeMap<usize, usize> = copies.into_iter().collect();
    for (i, f) in seen.iter().enumerate() {
        // Copies are moved wherever they are; everything else only from the
        // top of the folder (the folders inside are yours already).
        if let Some(keep) = copy_of.get(&i) {
            if open_elsewhere(&f.path, f.modified, now) {
                plan.in_use.push(f.path.clone());
                continue;
            }
            plan.moves.push(SortMove {
                from: f.path.clone(),
                into: review.join("Duplicates"),
                reason: Reason::Copy { of: seen[*keep].path.clone() },
            });
            continue;
        }
        if !f.top || plan.copies_only {
            continue;
        }
        let name = f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let ext = f.path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        let Some(mut kind) = kind_for(&ext) else {
            plan.unplaced.push(f.path.clone());
            continue;
        };
        // A program outside Downloads that doesn't call itself an installer
        // is one you may run from where it is.
        if ext == "exe" && !is_downloads && !looks_like_installer(&name) {
            plan.unplaced.push(f.path.clone());
            continue;
        }
        // A disc image is an installer only in Downloads.
        if ext == "iso" && !is_downloads {
            kind = FileKind::Archives;
        }
        if open_elsewhere(&f.path, f.modified, now) {
            plan.in_use.push(f.path.clone());
            continue;
        }
        let age_days = now.saturating_sub(f.modified) / 86_400;
        if kind == FileKind::Installers && age_days >= OLD_INSTALLER_DAYS {
            plan.moves.push(SortMove { from: f.path.clone(), into: review.join("Old installers"), reason: Reason::OldInstaller });
            continue;
        }
        if into.is_none() && already_home(dir, kind) {
            continue;
        }
        let year = (age_days >= OLD_FILE_DAYS).then(|| crate::civil::Civil::from_local(f.modified as i64).year);
        let mut to = dest_root.join(kind.folder());
        if let Some(y) = year {
            to = to.join(y.to_string());
        }
        plan.moves.push(SortMove { from: f.path.clone(), into: to, reason: Reason::Kind { kind, year } });
    }
}

/// The plan for these folders: nothing is moved. `into` is a folder named
/// to put the sorted folders in; otherwise each is sorted inside itself.
/// `budget` bounds the look; a folder too big for it is said to be.
pub fn plan_folders(folders: &[PathBuf], into: Option<&Path>, copies_only: bool, now: u64, budget: std::time::Duration) -> SortPlan {
    let until = std::time::Instant::now() + budget;
    let mut plan = SortPlan { folders: folders.to_vec(), complete: true, copies_only, ..Default::default() };
    if let Some(dest) = into {
        if let Some(from) = folders.first() {
            if let Err(why) = crate::tune::may_move_into(dest, from) {
                plan.refused.push((dest.to_path_buf(), why));
                return plan;
            }
        }
    }
    for dir in folders {
        if let Some(why) = refuse_to_sort(dir) {
            plan.refused.push((dir.clone(), why));
            continue;
        }
        plan_one(dir, into, now, until, &mut plan);
    }
    if plan.moves.len() > MOST_AT_ONCE {
        plan.more_next_time = plan.moves.len() - MOST_AT_ONCE;
        plan.moves.truncate(MOST_AT_ONCE);
    }
    plan
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string())
}

fn examples(paths: &[&Path], n: usize) -> String {
    let names: Vec<String> = paths.iter().take(n).map(|p| name_of(p)).collect();
    names.join(", ")
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The plan said in plain sentences, with counts and a few names, ending
/// with the question when there is something to move.
pub fn plan_said(plan: &SortPlan) -> String {
    let mut out: Vec<String> = Vec::new();
    for (_, why) in &plan.refused {
        let mut c = why.chars();
        let why = c.next().map(|f| format!("{}{}", f.to_uppercase(), c.as_str())).unwrap_or_default();
        out.push(format!("{why}, so I won't sort it."));
    }
    let where_ = {
        let names: Vec<String> = plan.folders.iter().filter(|f| !plan.refused.iter().any(|(r, _)| r == *f)).map(|f| name_of(f)).collect();
        match names.len() {
            0 => String::new(),
            1 => names[0].clone(),
            _ => format!("{} and {}", names[..names.len() - 1].join(", "), names[names.len() - 1]),
        }
    };
    if where_.is_empty() {
        return out.join(" ");
    }
    let mut by_kind: BTreeMap<FileKind, Vec<&Path>> = BTreeMap::new();
    let mut copies: Vec<&SortMove> = Vec::new();
    let mut installers: Vec<&Path> = Vec::new();
    let mut old = 0usize;
    for m in &plan.moves {
        match &m.reason {
            Reason::Kind { kind, year } => {
                by_kind.entry(*kind).or_default().push(&m.from);
                if year.is_some() {
                    old += 1;
                }
            }
            Reason::Copy { .. } => copies.push(m),
            Reason::OldInstaller => installers.push(&m.from),
        }
    }
    let sorted: usize = by_kind.values().map(Vec::len).sum();
    if sorted > 0 {
        let parts: Vec<String> = by_kind
            .iter()
            .map(|(k, v)| format!("{} {} ({})", v.len(), k.plural(), examples(v, 2)))
            .collect();
        out.push(format!("In {where_} I'd sort {} into a folder for each kind: {}.", count(sorted, "file", "files"), parts.join(", ")));
        if old > 0 {
            out.push(format!("{} untouched for over a year go into a year folder inside {}.", old, if by_kind.len() == 1 { "it" } else { "each" }));
        }
    } else if !plan.copies_only {
        out.push(format!("In {where_} there are no loose files I'd sort into folders."));
    }
    if !copies.is_empty() {
        let ex = copies
            .iter()
            .take(2)
            .map(|m| match &m.reason {
                Reason::Copy { of } => format!("{} is the same as {}", name_of(&m.from), name_of(of)),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("; ");
        out.push(format!("{} {} a copy of another file ({ex}).", count(copies.len(), "file", "files"), if copies.len() == 1 { "is" } else { "are" }));
    } else if plan.copies_only {
        out.push(format!("I found no copies of the same file in {where_}."));
    }
    if !installers.is_empty() {
        out.push(format!(
            "{} more than a month old ({}).",
            count(installers.len(), "installer", "installers"),
            examples(&installers, 2)
        ));
    }
    if !copies.is_empty() || !installers.is_empty() {
        out.push(format!("Those go into \"{TO_REVIEW}\" -- look through it and empty it yourself when you're happy; I never delete."));
    }
    let mut left: Vec<String> = Vec::new();
    if !plan.unplaced.is_empty() && !plan.copies_only {
        let v: Vec<&Path> = plan.unplaced.iter().map(PathBuf::as_path).collect();
        left.push(format!("{} I don't have a confident home for ({})", count(v.len(), "file", "files"), examples(&v, 2)));
    }
    if !plan.in_use.is_empty() {
        left.push(format!("{} open in another program or still being saved", count(plan.in_use.len(), "file", "files")));
    }
    if plan.passed_over > 0 && !plan.copies_only {
        left.push("hidden and system files, shortcuts and unfinished downloads".into());
    }
    if !left.is_empty() {
        out.push(format!("I'd leave {} where they are.", left.join(", ")));
    }
    if !plan.empty_folders.is_empty() {
        let v: Vec<&Path> = plan.empty_folders.iter().map(PathBuf::as_path).collect();
        out.push(format!("{} empty ({}) -- I leave those.", count(v.len(), "folder is", "folders are"), examples(&v, 2)));
    }
    if plan.more_next_time > 0 {
        out.push(format!("That's the first {MOST_AT_ONCE}; {} more next time you ask.", plan.more_next_time));
    }
    if !plan.complete {
        out.push("I ran out of time before comparing everything, so there may be more copies.".into());
    }
    if plan.has_moves() {
        out.push("Nothing is deleted or written over, and \"undo that\" puts every file back. Go ahead?".into());
    }
    out.join(" ")
}

/// What carrying out a plan did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SortDone {
    /// Each file moved: (where it was, where it is).
    pub moved: Vec<(PathBuf, PathBuf)>,
    /// Folders this made, for undo to take away again once empty.
    pub made: Vec<PathBuf>,
    /// Each file not moved, with why.
    pub not: Vec<String>,
}

/// Carry the plan out: each move judged by `system::judge` (the switch,
/// the folders Atlas may work in), a file open elsewhere skipped, never over
/// a file already there (`tune::move_files_into` gives it " (2)"), and
/// nothing deleted.
pub fn carry_out_moves(plan: &SortPlan, sys: &crate::system::SystemConfig, now: u64) -> SortDone {
    carry_out_moves_recorded(plan, sys, now, &mut |_, _| Ok(()))
}

/// Production sorting records the actual destination before touching a file.
pub fn carry_out_moves_recorded(
    plan: &SortPlan,
    sys: &crate::system::SystemConfig,
    now: u64,
    before: &mut impl FnMut(&Path, &Path) -> Result<(), String>,
) -> SortDone {
    let mut done = SortDone::default();
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for m in &plan.moves {
        let name = name_of(&m.from);
        let change = crate::system::Change::MoveFile { from: m.from.display().to_string(), to: m.into.join(&name).display().to_string() };
        if let crate::system::Verdict::Refuse(why) = crate::system::judge(&change, sys) {
            done.not.push(format!("{name} ({why})"));
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&m.from) else {
            done.not.push(format!("{name} (it isn't there any more)"));
            continue;
        };
        if open_elsewhere(&m.from, secs(meta.modified()), now) {
            done.not.push(format!("{name} (open in another program)"));
            continue;
        }
        groups.entry(m.into.clone()).or_default().push(m.from.clone());
    }
    for (dir, files) in groups {
        // Each folder this is about to make, outermost first, for undo.
        let mut missing = Vec::new();
        let mut d = dir.as_path();
        while !d.exists() {
            missing.push(d.to_path_buf());
            match d.parent() {
                Some(p) => d = p,
                None => break,
            }
        }
        let (moved, not) = crate::tune::move_files_into_recorded(&files, &dir, before);
        if !moved.is_empty() || dir.exists() {
            for m in missing.into_iter().rev() {
                if !done.made.contains(&m) {
                    done.made.push(m);
                }
            }
        }
        done.moved.extend(moved);
        done.not.extend(not);
    }
    done
}

/// What was done, said.
pub fn done_said(plan: &SortPlan, done: &SortDone) -> String {
    let mut s = if done.moved.is_empty() {
        "Nothing moved.".to_string()
    } else {
        let review = done.moved.iter().filter(|(_, to)| to.components().any(|c| c.as_os_str() == TO_REVIEW)).count();
        let sorted = done.moved.len() - review;
        let mut parts = Vec::new();
        if sorted > 0 {
            parts.push(format!("{} into folders by kind", count(sorted, "file", "files")));
        }
        if review > 0 {
            parts.push(format!("{} into \"{TO_REVIEW}\"", review));
        }
        let names: Vec<String> = plan.folders.iter().map(|f| name_of(f)).collect();
        format!("Moved {} in {}. Nothing was deleted; say \"undo that\" to put every one back.", parts.join(" and "), names.join(", "))
    };
    if !done.not.is_empty() {
        let shown: Vec<&String> = done.not.iter().take(4).collect();
        let more = done.not.len().saturating_sub(shown.len());
        s.push_str(&format!(
            " Not moved: {}{}.",
            shown.iter().map(|x| x.as_str()).collect::<Vec<_>>().join("; "),
            if more > 0 { format!(" and {more} more") } else { String::new() }
        ));
    }
    s
}
