//! The Folded A is one mark (Eric, 27 Sep 2026): `src/mark.rs` holds its
//! shapes, and every other copy (the files `design/mark/make_marks.py`
//! writes for Windows, Android, iPhone and the web) must match it. If this
//! fails, run `python3 design/mark/make_marks.py` and commit what it writes.

fn read(p: &str) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn png_size(b: &[u8]) -> (u32, u32) {
    assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    let n = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    (n(16), n(20))
}

/// The alpha of an RGBA PNG's top-left pixel. The first pixel of the first
/// row is stored as it is under every PNG filter, so it's the fifth byte of
/// the inflated image data (after the row's filter byte).
fn image_corner_alpha(png: &[u8]) -> u8 {
    // An opaque image may be saved without an alpha channel at all (colour
    // type 2): then every pixel is fully opaque.
    if png[25] == 2 {
        return 255;
    }
    assert_eq!(png[25], 6, "not an RGB or RGBA PNG");
    let mut data = Vec::new();
    let mut i = 8;
    while i + 8 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
        if &png[i + 4..i + 8] == b"IDAT" {
            data.extend_from_slice(&png[i + 8..i + 8 + len]);
        }
        i += 12 + len;
    }
    miniz_oxide::inflate::decompress_to_vec_zlib(&data).expect("the image data")[4]
}

#[test]
fn the_mark_files_are_the_mark() {
    assert_eq!(String::from_utf8(read("design/mark/folded-a.svg")).unwrap(), atlas::mark::svg_for_test(&atlas::mark::WARM_PAPER));
    assert_eq!(String::from_utf8(read("design/mark/folded-a-dark.svg")).unwrap(), atlas::mark::svg_for_test(&atlas::mark::EMBER_DARK));
}

#[test]
fn every_platform_has_its_icon_at_the_sizes_it_asks_for() {
    // The web app.
    for (f, s) in [
        ("assets/icon-192.png", 192),
        ("assets/icon-512.png", 512),
        ("assets/icon-maskable-512.png", 512),
        ("assets/apple-touch-icon.png", 180),
        ("assets/mark-256.png", 256),
    ] {
        assert_eq!(png_size(&read(f)), (s, s), "{f}");
    }
    // The rounded tile (Eric chose it): the web app's own icons have clear
    // corners; the maskable one is square for the system to cut.
    let corner = |f: &str| image_corner_alpha(&read(f));
    assert_eq!(corner("assets/icon-512.png"), 0, "the web app's icon lost its rounded corners");
    assert_eq!(corner("assets/icon-maskable-512.png"), 255, "the maskable icon must be full bleed");
    let manifest = atlas::hub::manifest("t");
    assert!(manifest.contains("icon-maskable-512.png") && manifest.contains("\"purpose\":\"maskable\""));
    assert!(atlas::hub::public_file("GET", "/hub/icon-maskable-512.png").is_some());
    // iPhone and iPad: light, dark and tinted, each 1024, and the project uses them.
    let dir = "mobile/ios/Atlas/Assets.xcassets/AppIcon.appiconset";
    let contents: serde_json::Value = serde_json::from_slice(&read(&format!("{dir}/Contents.json"))).unwrap();
    let files: Vec<&str> = contents["images"].as_array().unwrap().iter().map(|i| i["filename"].as_str().unwrap()).collect();
    assert_eq!(files.len(), 3);
    for f in files {
        assert_eq!(png_size(&read(&format!("{dir}/{f}"))), (1024, 1024), "{f}");
    }
    // Eric: the colour follows each person's setting. Every appearance is a
    // whole tile (iOS rounds it), never a mark floating on nothing.
    for f in ["icon-1024.png", "icon-1024-dark.png", "icon-1024-tinted.png"] {
        assert_eq!(corner(&format!("{dir}/{f}")), 255, "{f} isn't a whole tile");
    }
    assert!(String::from_utf8(read("mobile/ios/project.yml")).unwrap().contains("ASSETCATALOG_COMPILER_APPICON_NAME: AppIcon"));
    // Atlas's own windows: a light and a dark icon, both the rounded tile.
    assert_eq!(png_size(&read("assets/mark-256-dark.png")), (256, 256));
    assert_eq!(corner("assets/mark-256-dark.png"), 0, "the dark window icon lost its rounded corners");
    // Android: the adaptive icon, its themed layer, and the notification icon, all used.
    let res = "mobile/android/app/src/main/res";
    let adaptive = String::from_utf8(read(&format!("{res}/mipmap-anydpi-v26/ic_launcher.xml"))).unwrap();
    for part in ["@drawable/ic_launcher_foreground", "@drawable/ic_launcher_monochrome", "@android:color/transparent"] {
        assert!(adaptive.contains(part), "{part}");
    }
    // The rounded square Eric chose, drawn in the icon itself so no launcher's
    // mask (circle, squircle) can change it: the tile, then the mark.
    let fg = String::from_utf8(read(&format!("{res}/drawable/ic_launcher_foreground.xml"))).unwrap();
    assert_eq!(fg.matches("android:pathData").count(), 5, "the tile, front, back, fold and the dot");
    assert!(fg.contains(&atlas::mark::PAPER.replace('#', "#FF")), "the tile isn't paper");
    for c in [atlas::mark::WARM_PAPER.front, atlas::mark::WARM_PAPER.back, atlas::mark::WARM_PAPER.fold] {
        assert!(fg.contains(c), "the launcher icon isn't in the mark's colours: {c}");
    }
    // In dark mode Android takes the night version by the phone's own setting:
    // the Ember tile, the mark in its dark colours, still the rounded square.
    let night = String::from_utf8(read(&format!("{res}/drawable-night/ic_launcher_foreground.xml"))).unwrap();
    assert_eq!(night.matches("android:pathData").count(), 5);
    assert!(night.contains(&atlas::mark::EMBER.replace('#', "#FF")), "the night tile isn't Ember");
    for c in [atlas::mark::EMBER_DARK.front, atlas::mark::EMBER_DARK.back, atlas::mark::EMBER_DARK.fold] {
        assert!(night.contains(c), "the night icon isn't in the dark colours: {c}");
    }
    let manifest = String::from_utf8(read("mobile/android/app/src/main/AndroidManifest.xml")).unwrap();
    assert!(manifest.contains("android:icon=\"@mipmap/ic_launcher\"") && !manifest.contains("sym_def_app_icon"));
    let service = String::from_utf8(read("mobile/android/app/src/main/java/app/atlas/AtlasService.kt")).unwrap();
    assert!(!service.contains("android.R.drawable"), "a notification still wears a system icon");
}

#[test]
fn atlas_exe_carries_the_icon_and_its_name_for_both_windows_linkers() {
    let ico = read("assets/atlas.ico");
    assert_eq!(&ico[..4], &[0, 0, 1, 0], "not an .ico");
    let frames = u16::from_le_bytes([ico[4], ico[5]]);
    assert!(frames >= 8, "Windows asks for 16 up to 256: {frames} sizes");
    // The largest frame's bytes are inside both compiled resources.
    let last = 6 + 16 * (frames as usize - 1);
    let len = u32::from_le_bytes(ico[last + 8..last + 12].try_into().unwrap()) as usize;
    let off = u32::from_le_bytes(ico[last + 12..last + 16].try_into().unwrap()) as usize;
    let image = &ico[off..off + len];
    let name: Vec<u8> = "Atlas".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    for f in ["windows/atlas.res", "windows/atlas-res.o"] {
        let r = read(f);
        assert!(r.windows(image.len()).any(|w| w == image), "{f} holds an older icon: run make_marks.py");
        assert!(r.windows(name.len()).any(|w| w == name.as_slice()), "{f} doesn't name the program Atlas");
    }
    let build = String::from_utf8(read("build.rs")).unwrap();
    assert!(build.contains("atlas.res") && build.contains("atlas-res.o") && build.contains("rustc-link-arg-bins"));
    // The version Windows shows is the crate's.
    let rc = String::from_utf8(read("windows/atlas.rc")).unwrap();
    assert!(rc.contains(&format!("\"FileVersion\", \"{}\"", env!("CARGO_PKG_VERSION"))), "windows/atlas.rc is for another version");
}
