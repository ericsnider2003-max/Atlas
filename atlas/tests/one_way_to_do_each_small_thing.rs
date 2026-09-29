//! Small things that were done several ways, now done one way (27 Sep 2026).
//!
//! * **Finding a window by its process name.** Five places wrote an `AppSpec`
//!   out as YAML text and parsed it back. A name with a `"` in it broke the
//!   YAML: in `atlas window read` / `atlas window type` that was a panic
//!   (`.expect("spec")`), in the daemon a window quietly never found.
//!   `AppSpec::for_process` builds it directly.
//! * **Escaping text into HTML or XML.** `toast.rs`, `phoneadd.rs` and
//!   `ota.rs` each had their own escaper; two of them let `'` through. They
//!   all use `hub::esc` now.

use atlas::config::AppSpec;

#[test]
fn a_window_spec_takes_any_process_name_literally() {
    for name in ["notepad.exe", "say \"hi\".exe", "a: b", "[x]", "it's.exe", "back\\slash.exe"] {
        let spec = AppSpec::for_process(name);
        assert_eq!(spec.process_names, vec![name.to_string()], "{name}");
        assert!(!spec.store && spec.args.is_empty() && spec.title_hints.is_empty());
        assert!(!spec.no_input && !spec.docked_only);
    }
}

#[test]
fn the_built_spec_is_the_one_the_yaml_used_to_make() {
    // For an ordinary name the old route and the new one agree field for
    // field, defaults included -- so the change is the quoting and nothing
    // else.
    let parsed: AppSpec = serde_yaml::from_str(
        "launch: x\nprocess_names: [\"notepad.exe\"]\nrole: main\nlayout: full\n",
    )
    .unwrap();
    let built = AppSpec::for_process("notepad.exe");
    assert_eq!(format!("{parsed:?}"), format!("{built:?}"));
}

#[test]
fn the_old_yaml_route_really_broke_on_a_quote() {
    // Why the constructor exists: this is the text the five call sites built.
    let name = "say \"hi\".exe";
    let yaml = format!("launch: x\nprocess_names: [\"{name}\"]\nrole: main\nlayout: full\n");
    let old: Result<AppSpec, _> = serde_yaml::from_str(&yaml);
    assert!(
        old.map(|s| s.process_names != vec![name.to_string()]).unwrap_or(true),
        "the YAML route handled a quote after all -- the constructor's reason is gone"
    );
}

#[test]
fn a_notification_escapes_an_apostrophe_and_everything_else() {
    let x = atlas::toast::xml("Tom's <build>", "a & \"b\"");
    assert!(x.contains("Tom&#39;s &lt;build&gt;"), "{x}");
    assert!(x.contains("a &amp; &quot;b&quot;"), "{x}");
    assert!(!x.contains("Tom's"), "{x}");
}

#[test]
fn the_install_manifest_escapes_and_reads_back_an_apostrophe() {
    let ipa = atlas::ota::Ipa {
        bundle_id: "com.example.atlas".into(),
        build: "7".into(),
        version: "1.0".into(),
        title: "Eric's \"Atlas\" & <more>".into(),
        devices: Vec::new(),
        expires: None,
    };
    let m = atlas::ota::manifest_plist(&ipa, "https://x.example/a.ipa?x=1&y=2");
    assert!(m.contains("<string>Eric&#39;s &quot;Atlas&quot; &amp; &lt;more&gt;</string>"), "{m}");
    assert!(m.contains("a.ipa?x=1&amp;y=2"), "{m}");

    // What this escapes, the plist reader reads back -- `&#39;` as well as
    // the `&apos;` Apple's own files use.
    let back = atlas::ota::xml_plist(
        "<plist><dict><key>a</key><string>It&#39;s</string><key>b</key><string>It&apos;s</string></dict></plist>",
    );
    for k in ["a", "b"] {
        match back.get(k) {
            Some(atlas::ota::Value::Str(s)) => assert_eq!(s, "It's", "{k}"),
            other => panic!("{k}: {other:?}"),
        }
    }
}
