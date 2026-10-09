//! Documents, scans and zips read off the daemon's loop.
//!
//! Moved out of `daemon.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl FileDone {
    pub(super) fn no(said: String) -> FileDone {
        FileDone::Said { said, ok: false, log: None }
    }
}

/// Scan, then say whether it may be opened: `Ok` with a note to add (when
/// you said to open it anyway), or what to say instead.
fn scanned_ok_until(path: &std::path::Path, what: &str, anyway: bool, tools: &crate::voice::ToolsConfig, stop: &dyn Fn() -> bool) -> std::result::Result<Option<String>, FileDone> {
    if stop() { return Err(FileDone::no("Stopped before opening the file.".into())); }
    if anyway {
        return Ok(Some("not scanned — you said to open it anyway".into()));
    }
    let verdict = crate::unpack::scan_stoppable(path, &tools.files.virus_scan, stop);
    if stop() { return Err(FileDone::no("Stopped during the scan; the file was left unopened.".into())); }
    match verdict {
        crate::unpack::Verdict::Clean => Ok(None),
        crate::unpack::Verdict::Threat(_) => Err(FileDone::Said {
            said: format!("{}: {}.", path.display(), verdict.said()),
            ok: false,
            log: Some(format!("virus scan: {} — {}", path.display(), verdict.said())),
        }),
        crate::unpack::Verdict::NotScanned(_) => Err(FileDone::Ask {
            what: what.to_string(),
            path: path.display().to_string(),
            question: format!("{}. Open it anyway?", verdict.said()),
        }),
    }
}

/// Read a PDF or Word file, after scanning it. A scanned PDF is read page
/// by page with the word reader. Runs on the crew's thread.
pub(super) fn read_document_off(path: &str, anyway: bool, tools: &crate::voice::ToolsConfig) -> FileDone {
    read_document_until(path, anyway, tools, &|| false)
}
pub(super) fn read_document_until(path: &str, anyway: bool, tools: &crate::voice::ToolsConfig, stop: &dyn Fn() -> bool) -> FileDone {
    let p = std::path::PathBuf::from(path);
    if !p.is_file() {
        return FileDone::no(format!("I can't find {path}."));
    }
    let note = match scanned_ok_until(&p, "read", anyway, tools, stop) {
        Ok(n) => n,
        Err(done) => return done,
    };
    let lower = path.to_lowercase();
    let read: std::result::Result<(usize, String), String> = (|| {
        Ok(if lower.ends_with(".docx") {
            (0, crate::unpack::docx_text(&p)?)
        } else if lower.ends_with(".pdf") {
            let bytes = std::fs::read(&p).map_err(|e| format!("I couldn't open it: {e}"))?;
            let pdf = crate::pdftext::read(&bytes).map_err(|e| format!("I couldn't read it: {e}."))?;
            let scan = crate::files::pdf_is_really_a_scan(pdf.text_chars(), pdf.pages) || !crate::pdftext::looks_like_words(&pdf.text);
            if scan {
                (pdf.pages, read_pdf_photos_until(&pdf, tools, stop)?)
            } else {
                (pdf.pages, pdf.text)
            }
        } else {
            (0, std::fs::read_to_string(&p).map_err(|e| format!("I couldn't open it: {e}"))?)
        })
    })();
    let (pages, text) = match read {
        Ok(r) => r,
        Err(why) => return FileDone::no(why),
    };
    if text.trim().is_empty() {
        return FileDone::no("I opened it and there's no text in it I can read.".into());
    }
    // The whole text is kept beside Atlas's other readings, and the start
    // is said. Reading forty pages aloud is not what "read this" means.
    let dir = crate::roots::data_sub("reading");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return FileDone::no(format!("I read the document, but couldn't keep its text: {e}."));
    }
    let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
    let kept = dir.join(format!("{name}.txt"));
    if let Err(e) = crate::store::write_owned_file_until(&kept, text.as_bytes(), stop) {
        return FileDone::no(format!("I read the document, but couldn't save its text: {e}."));
    }
    let words = text.split_whitespace().count();
    let start = crate::research::first_sentences(&text, 4);
    let size = if pages > 0 { format!("{pages} page{}, {words} words", if pages == 1 { "" } else { "s" }) } else { format!("{words} words") };
    let scanned = match note {
        Some(n) => format!(" ({n})"),
        None => String::new(),
    };
    FileDone::Said {
        said: format!("{name}: {size}{scanned}. It starts: {start} The whole text is in {}.", kept.display()),
        ok: true,
        log: None,
    }
}

/// A PDF that's photos of pages: each photo through the word reader.
fn read_pdf_photos_until(pdf: &crate::pdftext::Pdf, tools: &crate::voice::ToolsConfig, stop: &dyn Fn() -> bool) -> std::result::Result<String, String> {
    if stop() { return Err("Stopped before reading scanned pages.".into()); }
    if pdf.images.is_empty() {
        return Err("It's a scan, but the pages aren't stored as photos I can read.".into());
    }
    let models = std::path::PathBuf::from(&tools.models.dir);
    if !crate::words::Reader::installed(&models) {
        return Err("It's a scanned PDF, and the two reading models that read scans aren't installed. \
                    They're an optional one-off download, and Atlas's window doesn't offer it yet."
            .into());
    }
    let mut reader = crate::words::Reader::open(&models).map_err(|e| format!("I couldn't start the reader: {e}"))?;
    struct Pages(std::path::PathBuf);
    impl Drop for Pages { fn drop(&mut self) { crate::heard!(std::fs::remove_dir_all(&self.0)); } }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let path = crate::roots::tmp_dir().join(format!("atlas-pdf-pages-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&path).map_err(|e| format!("I couldn't prepare scanned pages: {e}"))?;
    let dir = Pages(path);
    let mut out = Vec::new();
    for (i, jpg) in pdf.images.iter().enumerate() {
        if stop() { return Err("Stopped between scanned pages; no complete reading was saved.".into()); }
        let f = dir.0.join(format!("page-{}.jpg", i + 1));
        std::fs::write(&f, jpg).map_err(|e| format!("I couldn't prepare page {}: {e}; no complete reading was saved.", i + 1))?;
        let read = crate::words::read_file(&mut reader, &tools.video.ffmpeg, &tools.vars, &f.display().to_string(), &tools.words)
            .map_err(|e| format!("I couldn't read page {}: {e}; no complete reading was saved.", i + 1))?;
        out.push(read.text());
        crate::heard!(std::fs::remove_file(&f));
        if stop() { return Err("Stopped after reading the current page; no complete reading was saved.".into()); }
    }
    let text = out.join("\n\n");
    if text.trim().is_empty() {
        return Err("It's a scan, and I couldn't make out the writing on its pages.".into());
    }
    Ok(text)
}

/// Unpack a zip beside itself, scanning it first and what came out after.
pub(super) fn unzip_until(path: &str, anyway: bool, tools: &crate::voice::ToolsConfig, stop: &dyn Fn() -> bool) -> FileDone {
    let p = std::path::PathBuf::from(path);
    if !p.is_file() {
        return FileDone::no(format!("I can't find {path}."));
    }
    let note = match scanned_ok_until(&p, "unzip", anyway, tools, stop) {
        Ok(n) => n,
        Err(done) => return done,
    };
    let dest = crate::unpack::folder_beside(&p);
    let files = match crate::unpack::unzip_stoppable(&p, &dest, &tools.files, stop) {
        Ok(f) => f,
        Err(why) => return FileDone::no(format!("Unpacking did not finish: {why}. Partial files may remain in {}. The original archive is untouched.", dest.display())),
    };
    // Everything it unpacked to, scanned as one folder.
    if stop() { return FileDone::no(format!("Unpacked {} files into {}, then stopped before scanning them. Don't open them until they are checked.", files.len(), dest.display())); }
    let after = crate::unpack::scan_stoppable(&dest, &tools.files.virus_scan, stop);
    let safe_to_continue = matches!(&after, crate::unpack::Verdict::Clean);
    let n = files.len();
    let head = format!("Unpacked {n} file{} into {}", if n == 1 { "" } else { "s" }, dest.display());
    let said = match (after, note) {
        (crate::unpack::Verdict::Clean, None) => format!("{head}, and Windows Defender found nothing in them."),
        (crate::unpack::Verdict::Threat(t), _) => format!(
            "{head}, and Windows Defender found {t} among them. Don't open them — Defender's own screen can quarantine it."
        ),
        (crate::unpack::Verdict::Clean, Some(n)) => format!("{head} ({n}); the extracted files were checked and no threats were found."),
        (crate::unpack::Verdict::NotScanned(why), _) => format!("{head}, but I couldn't scan what came out ({why}). Dependent work is stopped; check these files before opening them."),
    };
    FileDone::Said { said, ok: safe_to_continue, log: None }
}

#[cfg(test)]
mod extracted_file_prerequisites {
    use super::*;
    #[test]
    fn an_archive_override_does_not_mark_unchecked_extracted_files_successful() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("atlas-unzip-prerequisite-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let archive = root.join("photos.zip");
        std::fs::copy("tests/fixtures/documents/photos.zip", &archive).unwrap();
        let before = std::fs::read(&archive).unwrap();
        let mut tools = crate::config::Config::load(std::path::Path::new("config")).unwrap().tools.unwrap();
        tools.files.virus_scan.command.clear();
        let result = unzip_until(&archive.display().to_string(), true, &tools, &|| false);
        match result {
            FileDone::Said { said, ok, .. } => {
                assert!(!ok, "unchecked extraction must hold its prerequisite");
                assert!(said.contains("Dependent work is stopped"), "{said}");
            }
            _ => panic!("expected explicit extracted-file outcome"),
        }
        assert!(root.join("photos/trip/notes.txt").is_file());
        assert_eq!(std::fs::read(&archive).unwrap(), before);
        std::fs::remove_dir_all(root).unwrap();
    }
}
