//! Research write-ups as Word and PDF files (`report`, 1 Oct 2026).

const NOTE: &str = "# Heat pumps in cold climates

Modern cold-climate heat pumps keep **most of their output** down to -15 C, according to NEEP's list [1]. Below that, a backup heater usually runs.

## What the sources say

- NEEP lists units rated at 100% capacity at 5 F.
- The Department of Energy says *ductless* units save 30-50% on heating \u{2014} \u{201C}compared with electric resistance\u{201D}.
1. Size it for the coldest week.
2. Keep the backup heater.

A very long address that must still fit on the page: https://www.example.org/a/very/long/path/that/goes/on/and/on/and/on/and/on/and/on/and/on

## Sources
- https://neep.org/heating-electrification/ccashp-specification-product-list
- https://www.energy.gov/energysaver/heat-pump-systems
";

#[test]
fn the_write_up_reads_as_pieces() {
    use atlas::report::Block;
    let b = atlas::report::blocks(NOTE);
    assert_eq!(b[0], Block::Title("Heat pumps in cold climates".into()));
    assert!(b.contains(&Block::Heading("What the sources say".into())));
    assert!(b.contains(&Block::Numbered("2.".into(), "Keep the backup heater.".into())));
    assert!(b.contains(&Block::Source(2, "https://www.energy.gov/energysaver/heat-pump-systems".into())));
    assert_eq!(b.iter().filter(|x| matches!(x, Block::Bullet(_))).count(), 2);
}

#[test]
fn the_pdf_reads_back_with_its_words_and_links() {
    let bytes = atlas::report::pdf(NOTE).unwrap();
    let back = atlas::pdftext::read(&bytes).unwrap();
    let text = back.text.replace('\n', " ");
    for want in ["Heat pumps in cold climates", "most of their output", "Size it for the coldest week", "Page 1 of 1"] {
        assert!(text.contains(want), "{want:?} not in {text:?}");
    }
    let raw = String::from_utf8_lossy(&bytes);
    assert_eq!(raw.matches("/S /URI").count(), 2, "each source is a link");
    assert!(raw.contains("/Helvetica-Bold"));
}

#[test]
fn a_long_write_up_runs_onto_more_pages() {
    let long = format!("# Long\n\n{}", "A paragraph of ordinary words that goes on for a while. ".repeat(40).repeat(1) + "\n\n").repeat(12);
    let bytes = atlas::report::pdf(&long).unwrap();
    let back = atlas::pdftext::read(&bytes).unwrap();
    assert!(back.text.contains("Page 1 of") && back.text.contains("Page 3 of"), "{}", &back.text[..200.min(back.text.len())]);
}

#[test]
fn the_docx_is_a_word_package_with_styles_and_links() {
    let bytes = atlas::report::docx(NOTE);
    let get = |name: &'static str| {
        atlas::zipread::file_inside(&bytes, |n| n == name, 1 << 20).unwrap().map(|(_, b)| String::from_utf8(b).unwrap()).unwrap_or_else(|| panic!("{name} missing"))
    };
    let doc = get("word/document.xml");
    assert!(doc.contains("<w:pStyle w:val=\"Title\"/>") && doc.contains("<w:pStyle w:val=\"Heading1\"/>"));
    assert!(doc.contains("<w:b/></w:rPr><w:t xml:space=\"preserve\">most of their output"));
    assert!(doc.contains("\u{201C}compared with electric resistance\u{201D}"), "punctuation kept");
    assert_eq!(doc.matches("<w:hyperlink").count(), 2);
    let rels = get("word/_rels/document.xml.rels");
    assert!(rels.contains("TargetMode=\"External\"") && rels.contains("neep.org"));
    assert!(get("[Content_Types].xml").contains("wordprocessingml.document.main+xml"));
}

#[test]
fn the_file_lands_beside_the_write_up() {
    let dir = std::env::temp_dir().join(format!("atlas-report-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let md = dir.join("1759300000-heat-pumps.md");
    std::fs::write(&md, NOTE).unwrap();
    let out = atlas::report::export(&md, atlas::report::Kind::Pdf).unwrap();
    assert_eq!(out, dir.join("1759300000-heat-pumps.pdf"));
    let out = atlas::report::export(&md, atlas::report::Kind::Word).unwrap();
    assert_eq!(out.extension().unwrap(), "docx");
    if let Ok(keep) = std::env::var("ATLAS_KEEP_REPORTS") {
        for e in ["pdf", "docx"] {
            std::fs::copy(md.with_extension(e), std::path::Path::new(&keep).join(format!("sample.{e}"))).unwrap();
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
