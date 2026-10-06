//! Personal Atlas knows nothing about Eric's trading system.
//!
//! Eric, 26 Sep 2026: "Atlas for personal/business use should have its own
//! versions without [the trading system's] information in it. Atlas as a
//! baseline can still have general trade identification, and trading
//! knowledge but nothing specific about [it] within it. When the … code is
//! removed it should know nothing about [it] unless myself or dev-2 teach it."
//!
//! That system has its own Atlas, with its own copy of what it needs, in its
//! own private folder. Nothing here may name it: not the code, the shipped
//! config, the tests, the docs, the design files or the phone apps. Personal
//! Atlas is what friends are given, so anything named here would be handed
//! out with it. What someone teaches their own Atlas at run time is theirs and
//! lives in their data, never in this tree.
//!
//! The name is built from pieces below so that this file does not itself
//! carry it.

fn the_name() -> String {
    ["escape", "mint"].concat()
}

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Build output and fetched tooling are not the tree.
        if matches!(name, "target" | "build" | ".gradle" | ".cxx" | "rustlib" | "node_modules" | ".git") {
            continue;
        }
        // The checkout's scratch (tests' and builds' temp files, ignored by
        // git; ledger Q21) is not the tree either.
        if name == "scratch" && p.join(".gitkeep").is_file() {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn nothing_in_personal_atlas_names_the_trading_system() {
    let needle = the_name();
    let mut files = Vec::new();
    for top in [".", "src", "config", "tests", "docs", "design", "mobile", "assets", "setup", "vendor"] {
        let d = std::path::Path::new(top);
        if top == "." {
            // Only the files at the top of the crate; the folders are walked below.
            for e in std::fs::read_dir(d).unwrap().flatten() {
                if e.path().is_file() {
                    files.push(e.path());
                }
            }
        } else {
            walk(d, &mut files);
        }
    }
    assert!(files.len() > 500, "the walk found only {} files", files.len());
    let mut named = Vec::new();
    for f in &files {
        let Ok(bytes) = std::fs::read(f) else { continue };
        let text = String::from_utf8_lossy(&bytes).to_lowercase();
        if text.contains(&needle) {
            named.push(f.display().to_string());
        }
    }
    assert!(
        named.is_empty(),
        "personal Atlas names the trading system in:\n  {}\n\nThat knowledge belongs with that \
         system's own Atlas, not in the Atlas that is handed to friends.",
        named.join("\n  ")
    );
}

#[test]
fn the_modules_that_judged_its_trades_are_gone() {
    // The machinery that compared Atlas's view with the trading system's own
    // decisions, joined their logs, and graded Atlas as its consultant. It
    // lives with that system now. `ladder` joined them on 28 Sep 2026: it
    // carried that system's own stop rules, not general trading knowledge.
    for m in GONE {
        assert!(
            !std::path::Path::new(&format!("src/{m}.rs")).exists(),
            "src/{m}.rs is back in personal Atlas"
        );
        assert!(
            !std::path::Path::new(&format!("tests/{m}.rs")).exists(),
            "tests/{m}.rs is back in personal Atlas"
        );
    }
    let lib = std::fs::read_to_string("src/lib.rs").unwrap();
    for m in GONE {
        assert!(!lib.contains(&format!("pub mod {m};")), "lib.rs declares {m} again");
    }
    // Its own Atlas and the files that ran beside it left the repository too
    // (28 Sep 2026). Checked only when the crate sits in that repository.
    if let Some(root) = repo_root() {
        for d in [format!("atlas-{}", the_name()), "engine-side".to_string()] {
            assert!(!root.join(&d).exists(), "{d}/ is back beside personal Atlas");
        }
    }
}

/// The modules that left, by name. Their names are not the secret; what they
/// held is, and that is what the fingerprint check below is for.
const GONE: [&str; 8] = ["apart", "inflight", "joined", "sealed", "concord", "marks", "replay", "ladder"];

// ---------------------------------------------------------------------------
// The fingerprint check (28 Sep 2026)
// ---------------------------------------------------------------------------
//
// Taking out the name was not enough. The 26 Sep pass removed every mention of
// it and left behind the system's own stop rules, figures from its trades, its
// people and machines, and sample data that described it. Eric, 28 Sep 2026:
// personal Atlas and the trading system's own Atlas "are meant to be
// separate. I don't want to send [that Atlas] with proprietary information to
// my friends."
//
// So this checks for what the system *is*, not only what it is called: its
// rule names, parameters, figures, accounts, people, machines and the phrasing
// of its rules. The list must not itself be in this tree, so only hashes are
// here. Each row is (FNV-1a-64 of the first word, FNV-1a-64 of the whole
// phrase, SHA-256 of the phrase); the plain list is kept with that system's
// own Atlas, beside the tool that makes this table.
//
// How a phrase is found, and why:
// * Lower case. Words are runs of letters, digits and `_`, joined through
//   `- . , '` when a word character follows (so a hyphenated figure, a
//   decimal, a thousands figure, a possessive and a snake_case name are each
//   one word). A second pass splits on those too, so a phrase inside a
//   hyphenated or snake_case name is still found.
// * Phrases of one to four words. Words join across whitespace, line breaks
//   and comment or list markers (`/ * # > - ! _`), so a phrase wrapped onto the
//   next line of a comment is still one phrase; they do not join across code
//   punctuation, which is what keeps `account: {e}` from reading as a phrase.
// * Every text file in the crate and, when the crate sits in its repository,
//   every text file beside it -- and every file's own path.
//
// To add a fingerprint: add the phrase to the plain list kept with the other
// system, run its make_guard.py, and paste the table here. Never type the
// phrase into this file.

const FINGERPRINTS: &[(u64, u64, &str)] = &[
    (0xcbf9fd7bcc3c3872, 0xcbf9fd7bcc3c3872, "b3f9facff38b5787af26aa50faf6c3dac259a84121bf053b984f1cc7038e02b2"),
    (0x32f90844a302d946, 0xc5a7e53160403e16, "e727bbadf4501d9b61e9f4722a659288c497b14891ff756478e16919d69b0823"),
    (0x48996f6e7b2a8a21, 0xd9c6346135cab22e, "f9e4ad1760bc6856a502670d51226833467f64c0d360a2c122f18b673d80e1fd"),
    (0x48996f6e7b2a8a21, 0xd67d5e2e676d1407, "cfd0ecff91296df673024a3ce58cd658648796a577e98364f7e93b6213021471"),
    (0xbd95e2529e216f1a, 0xb1e575a344bd99c3, "18fa4bb84af3b67c9c9efd3a2f23ebc3f9b191c4e94ed8aef93579338b9b0bc5"),
    (0xbd95e2529e216f1a, 0x0680946dce2c2610, "56d61c68a154931cfe8d20723eb4efc345cb18481bcd3704f5e3ef5893ee3f7d"),
    (0x0960b7ff6111d45a, 0x0960b7ff6111d45a, "67506dc60ec15b89dae2b435737ce92e7f571088c825d92a32eb643f57c9fe1c"),
    (0xc5ff26745a13649b, 0x4db5e26254f6d1f1, "50cebccd0e4dfa981206851a161437de3dfb463f2ae3dfd8d46488af6845a5e3"),
    (0x547141bd5790168a, 0x547141bd5790168a, "e6ccc2751bbc9c3c231901997d6339c0cf5e1923ebc0ebef886640dc266e5e6a"),
    (0xce4bedad712621ba, 0x6952e55ab5221180, "0f98db4dc7dd9b845377730cc4d744eb9b2e5e651cf6a4f456d3992cc124bcdc"),
    (0xd4400d1256d202f3, 0xd4400d1256d202f3, "ac69f67c0a7a8f5098db6a6dd8f53eff05d62869cdebf41f61ee5302eaba234e"),
    (0x2f768c575da87d46, 0x0836014842c55086, "96f39b0041f2b60667e6ad4a94b8b8fe6495e767a4722a4f348c53bbbaf43207"),
    (0x2d0ec1d4b5a0a143, 0x2d0ec1d4b5a0a143, "57239ed12dba7e250f00679e37065f4b5cc7518542dd661f1ecd0f033552bbc6"),
    (0xd66e3beb115e11aa, 0x0f6bcacfa7ab8652, "e6813fb14c8261527e991d3c923aa606a9ce94cb929aad5896a67e34d73fea28"),
    (0x9d162355f1f42139, 0x9d162355f1f42139, "5d0dde78cbe7966fdb6401619fdda74ec4967e97583d5928a31b4aa6009ccbe7"),
    (0x407efecc7eb5764f, 0xd2dd63edc7c25fc8, "dd56f4787e686c14a4d9c05a057e903a65f1107363df6105e7af8a0f6aace8bd"),
    (0xfa03d3fa28dafa01, 0xfa03d3fa28dafa01, "154dd52eda645d12cae027cbc9ba29d59dee11c85b1a91b8159a33ef0cba5525"),
    (0xea561e1e1cd7b19b, 0x4c71296cd16ccec3, "dcadf713f708610bc981d9aae886baa2dfb1f784f46965ae7d7490212e3e0183"),
    (0x042436aa358816a8, 0x042436aa358816a8, "4bc60649bc77df62a58d72c1759c064fc600d71b650da7af30d40d4e6c31ff04"),
    (0x0b278878dbe15ea7, 0x18de32199c0bd873, "8094b3ea7a32633ea3c3b981c677a18662d83f45dd095439bf4ca2a3581963dc"),
    (0xae33cbda58e985f6, 0xae33cbda58e985f6, "2a8831717a3032c8cf1e3c02d54256239414ea6d51f0cbcd39ba69d5f183f7b3"),
    (0x4b701c187218deca, 0xc018ce27bfce2485, "29ee96bf118271edfe81363cd7bc902687416532c10ec43946fb8089b7fc7a80"),
    (0x781eb00152d788b0, 0x781eb00152d788b0, "b62e8732178a693be01973eb50d09b1f037b0a28fca352f6ce12854c432cd201"),
    (0x05bd39f06f76d6bc, 0x38c8dce6bc71139c, "337696c46ba54144b1d558af16adbfc60b9a3a60ee313df1c53afb122745f324"),
    (0xfa4cf6ef19d2f987, 0x323d63d39a293c60, "b1b2c71de6ec5206f73b773f05ba487deda5b3b9544a38d1a4c6e23cebe3c4e0"),
    (0xcba3479fa891919d, 0xcba3479fa891919d, "aa01e22a31b75246ced3aa1d204c17561e3b287642c63acc5ce6628dc3d342ff"),
    (0x08140207b4d18eac, 0x2bdfdecb50b3e3ed, "2931a14ed0e7af5341581d354f66d8f30387745308b5e297dccd4ae5651e25c3"),
    (0x08140207b4d18eac, 0x414b317a21abc57a, "9765edf2af72ee64963dd6a75ec536f96e953d0d9889bed7adf030943c5f9eea"),
    (0xbe394dd9ff3ddc13, 0xbe394dd9ff3ddc13, "58365095e78903bb0099b6529d8f98470192bac3f76d63f4e3dfea7cfb51ad77"),
    (0xd34262a38125f3d3, 0xd34262a38125f3d3, "a2bcb0922b7c6c6f33f8b1285c0c34c18cae2117fffb51dd0974aff4b711bac6"),
    (0x82d5a63a93fc7d2d, 0x82d5a63a93fc7d2d, "d8b0dbdf19480299d8cb91b5a306d2697e7d5b28d857687b020d8ca5235c4880"),
    (0x7b08df8fe3fb3ebb, 0x7b08df8fe3fb3ebb, "9667bcafda5d460d13a9562339d47ff02af8b99cea022efb31556f6bbf94acff"),
    (0xbe638ef9583810c4, 0x1ee871394b9a3b40, "d05537bc0c08ca661eb239cc3df4045800d5beac2691a6d1efdc3138794a4dfa"),
    (0xbb3962b0e741ba2c, 0xb4398ae70fc46f52, "56a519324fbdb8d7d749796d6e08209e8f6985b3a2ff2a3ed89561d7c36363c9"),
    (0x456fc3181822c68e, 0x3f031b5481b08efc, "1441ad4ffcb2cb01008e77e767969cec7121435017b0be6acf29dbf149d40619"),
    (0xe6090add9002013a, 0x7496e14b090ccb41, "fd416f48afb188adce2e4ce9743f2fc3a0f278eac718700811beef97e8f4f530"),
    (0x56b5a72b50ecd75b, 0x719be13c8fdbb04f, "d3ec12ced79b1dbc3648fd2c0eb9836260ee9b4a81f04e7413c3f5aa7486599c"),
    (0x9a7ce19baa54c278, 0x52925cec8cd24aab, "486aa19bd64b0c19762c09d2ec4aa017ea9591e2894803bbc1479c1983ebac13"),
    (0xf5c792be2cb8f9ba, 0xf5c792be2cb8f9ba, "754aa794a81d851797497d507a6f3bae70aae6db3f7274d95766005e97ab3cce"),
    (0xa5f555674285a957, 0xa5f555674285a957, "cf4b9c1f5eb31deb9ea41f56faa757b68be9cab8b73f463229df17036bdfa13e"),
    (0xc479db0e57a43cb2, 0xc479db0e57a43cb2, "309a2907d1ca5a8a10da1893a18f001cd1da093f02b0f14fe4e63acc886abf36"),
    (0xc479db0e57a43cb2, 0xf04a2a57617e3463, "f1260e27283eda30f87a5cb5c6ae9b28084b6b58e8939e64278a0ff0e9cb4460"),
    (0x88f4585e5c6e9d5d, 0x88f4585e5c6e9d5d, "2a5bc324dbd8bf65c7c256b5e3823604d93048e93d71931c911801a420f033dd"),
    (0x37fcd52d58edadb2, 0x37fcd52d58edadb2, "2a2a514f166dc5b75f4075d21f374ed3aac317985c373a73307a0de0a47b85d6"),
    (0x0251fec64383f085, 0x8170933af57a04fe, "814e4540ceb3ef43d62a7a634f65e5f22a669c456bd9767900addf8265058bf7"),
    (0x0369250deb889a31, 0x83bc9ee4462ae25e, "a7946540ffe77bf35de2da5454f909cc8dff2b0232249a86659b2f61ae3d905b"),
    (0x0369250deb889a31, 0x04642ae33ade5277, "f94046d933a99dd0963c06424ec3a9a3f7c9a296e226fbf339950cf5008d7461"),
    (0xb828037e802c0581, 0xb828037e802c0581, "4a49ce1dad6f794ac85660f4cf292e80cf64756da615c12559c2c8bdf7fad578"),
    (0xa23fd2965dc429bc, 0xde8950c248889db6, "6d777c6b0755819269663a95bf44a7be7f3007bb2e1864cdd2cf2c659e397492"),
    (0xa23fd2965dc429bc, 0xde8951c248889f69, "5e4c1d5572ec37eddc078be655c7889ea6261887195ddb9460824b51af327f35"),
    (0xa23fd2965dc429bc, 0xde895ac24888aeb4, "9d69fc3d4733598a40893dacade157bcf327dae7efcf06ba6403499a2a0df9ed"),
    (0xdf7a5bc249553308, 0xdf7a5bc249553308, "b8329997ef278241df952868b91dc4caeffd8141d0731b5e2adb07dddd6d7a06"),
    (0xdf7a5ec249553821, 0xdf7a5ec249553821, "120e4091f28615f3ce66646897d79d664e7c2e9e00d1793a1b8acf9e84a17281"),
    (0xdf7a5dc24955366e, 0xdf7a5dc24955366e, "765d060e0befddd62aa187a5ace01f9ede365bd9ad89a50fe4bc13b84228f106"),
    (0xdf7a60c249553b87, 0xdf7a60c249553b87, "65ff7f7fdcf6bbe39093369887198d3daa489677b359636a8396ef9c2257b0ed"),
    (0xdf7a5fc2495539d4, 0xdf7a5fc2495539d4, "b2b9953ac2636333538da50242be550073ea2a7dac16a1106c9a8cdb31dc2d8a"),
    (0xdf7a62c249553eed, 0xdf7a62c249553eed, "bd9dd26e1c6770eeba0874714673463a020f99e1cfdbb016882ca37f4b015e64"),
    (0xdf7a61c249553d3a, 0xdf7a61c249553d3a, "56a7535df89de1ecc7dd3f40364b96bf711731c4d7c8f729db9ce95ca79d1d5b"),
    (0xdf7a54c249552723, 0xdf7a54c249552723, "95d5db95c97317ed35fd5ca5fde65122c048d2229f1774ed29a7366459238f2d"),
    (0x7da74b9803dd60c3, 0x66468680ad240ded, "b80381078c0c80bcc5f2717ac850e7c9da2fd82b7e8e6480758763090a268787"),
    (0x7da74b9803dd60c3, 0x66468580ad240c3a, "aa7ee661a2dfa432dade7cb4db3602b981dce12a70910b457d342a0b1cc968db"),
    (0x7da74b9803dd60c3, 0x66467880ad23f623, "cc2a34e9a05279a0010a54dc5bc7febf6992bc65907da07d3b7f911deacfe9d8"),
    (0xd102a6c7e0667b3f, 0xd102a6c7e0667b3f, "baa235949e1fa18894ede74bb7073aa4e2d26c9e3b14042d68025441be3e3761"),
    (0x14d828c76835886d, 0x14d828c76835886d, "6ce17efd4df47fc72475c6aee01cb25554cdd964492025040cd8b70017060926"),
    (0xcd848109a8a37011, 0xcd848109a8a37011, "1015284c363b0f12142d8ef1d8fb5ba26e7a358b3a4af517f09b2c2f0bb2d59a"),
    (0xdb08a73b44dadeb3, 0xdb08a73b44dadeb3, "2f7914d4b03abc1b539c28e6a24b50b3fe0416512ac75f23797101f7ea84c1c4"),
    (0xd87d1f54699022ef, 0xd87d1f54699022ef, "eef403d9a2178a782549987baea3f206618dca8f287e634d934cb5f06eb2115d"),
    (0x4da0a69c18378219, 0x4da0a69c18378219, "812b17a76009e6562029061a85cb3404d76ea0b0dd7a99c9ac0e62b3dae4b128"),
    (0x33c0780be423398d, 0x33c0780be423398d, "6799c6919a7e4bc0ed11dccc20aacaf7d37c4ac3e56fb78ed0c901c7f4386000"),
    (0x066e339ac3a34cc1, 0x066e339ac3a34cc1, "088d6c60357a08cb955c4906d6a665ace2564b11479c10c77294b81d46f8b553"),
    (0x3c0739ff2935e63d, 0x3c0739ff2935e63d, "c0c29d4c3b5e763f8813810c33451697621b49ca7113feeefaf57c674fee4bef"),
    (0x3e0c8918144ff8f6, 0xf99462fcfd925445, "61f7e01aa1df814cbcbb353e4523771b1cb48a0d944ed2eaa22da1b155b68764"),
    (0xcbd8031e3d026dc9, 0x8c4dfdaac9b01b63, "5e8032472a0602bd924405085461d7336898ed51862c1fc875eb65ce3a41a540"),
    (0xd7bae7e66a6099a9, 0xec5446f205cb5b35, "be7a694bede547a28c044e2ba6538c71956b66dc8a2faf595ac5073bcd4ef323"),
    (0x76aaaa535714d805, 0x9bd342d63b168d9b, "3b7a885dce406968e72082e7c10adb7d686dbc504ff66b26679ff6940da6ba00"),
    (0xbfc39ca80fc22cd0, 0xbfc39ca80fc22cd0, "e1ed6e1eec28934ca36d850e87e30afacd0d8856c9c9eca183cfe978125b6ef1"),
    (0xbd8fd7ffc962f5d8, 0xbd8fd7ffc962f5d8, "f693f251bcc40fc93d553a0cd4ceb51f39aee1706937471f507f0f435bfcfd0b"),
    (0x0af8a8ff626c87a5, 0xaf4b824908f237f6, "a79df3389225df3a64cc77da8472b12c2e82eb752856487589c1627b56503f28"),
    (0x56f5c9194461d57c, 0xbdaba493fb156a21, "e620d55a8cdbde86b31afcfc2e38bcb1a26eb72992ef1517ab11b2faab1e3036"),
    (0x3dacd07fc0cad3db, 0x3afeb3c573e60aeb, "ad696304c0ec667a41d6896a248c402cfba5c450060777938b8ad2a5460b537f"),
    (0x0af8a8ff626c87a5, 0x7eddc7b7efec4abc, "cad0aa43b52aeb4a477110bcc8890c8fb54a057069998cdfca0774043a6be462"),
    (0x0af8a8ff626c87a5, 0xfa64d83f5f2e3de4, "67054b02e797d2940fc80181313f46bab3838400d79637f7ca44c2aae217b94e"),
    (0x0af8a8ff626c87a5, 0xf442481393af2b80, "f81d111636c691da52cce72962f1bf20be8b2fd252618f4eeb4f4ad1eb5072cc"),
    (0x56f5c9194461d57c, 0x92dbf869f1551891, "f99b77dbe817a0996fc520e3802671f8c8c0bfa6e49260b2d5be68014969c29e"),
    (0x08a94307b5502113, 0x99f81d1ea9bf810b, "79911642990d1bdcf5984a8dbca5068c73e18b4885f1745bbe0daf96c832ec9a"),
    (0x70c68610ec47c2ea, 0x67b09db64a6b47db, "0601a059d0ed1926d6d54f33970b681f920574ef1780692d7e8e5c39dfa02381"),
    (0x5062cd8f7a7fd349, 0xa6b337ab8e05ec8a, "6d6c1e3c83c74afbf413fb17a766191245b39a3383ff3713a6039c1f7f34440f"),
    (0x2a500531bdce91cc, 0x227db15febdce649, "59596ad14c3ea8fc607b184a381f68f9d592a3a2fa587f4d8937a5c07f6ed57c"),
    (0xb22958606fce0f0a, 0xbf888fd91f5f3ecb, "d3bdf14fc60df102026c89f4f5cbd133b4cd242ce3715d220d4f9ced679a0e08"),
    (0xc9a9f34a688785ac, 0x44960a4c780d7cc8, "46bf83b21a24acb3247ddbb516f4299c47d8ac4ba6e714010f498bac0db7f589"),
    (0x37db9bddbd84fc81, 0xce2dfd49bff4f60c, "25c2cba72fb7a82c3bd5552b693ef22f7ca796a84f50561f7c080599ae033011"),
    (0xd690d04cca79a8aa, 0xd690d04cca79a8aa, "7d6ed7e72be2cd7bde24bfa7532ca7b2ba62ad812dc17883d3405fd87ff60cf6"),
    (0xa355141ff0c48eda, 0xd10e5853d2d33ba1, "a0b4ba5fbf0edb574e815ab9ef4477a002dfaf7965d81db9dcab4d003260fd25"),
    (0x0084407d9c3bb694, 0xd0fcce2d990b9703, "0eccdc5a436582c59889b3ace5d068582f61b6d68642b49e7fe8b4c76dcf370d"),
    (0xbe76d9d1594bba8f, 0xbe76d9d1594bba8f, "d3557c5b6e5ffbab92cba19172bb0bc65958980373b10a23a7aefcaa7c23a415"),
    (0xe2ee32f1d18f17ed, 0xe2ee32f1d18f17ed, "01c8b2335ddcbaa1eabdb3cc746f997400073214b5a51842f599ae4e3f2c913d"),
    (0x8c7b6e7174178420, 0x8c7b6e7174178420, "52db2b4a2c3da93e8d9602cc98e4491151a218d585c7e021e3e67ed773ecc613"),
    (0xcd40fdc843f5d909, 0xcd40fdc843f5d909, "a29b90056ab46255d7a860308114c16408533dfb0b6c0cc54270e6bd5cf9614d"),
    (0x8596fc76092a2e96, 0x8596fc76092a2e96, "0388fb626ca89a127847443989334b8c29e17567bc03a7a2ed13effca701a4a1"),
    (0x51a0d35fb5040388, 0x51a0d35fb5040388, "b523c7aedc9e279042105acbef28c4dd83e5a974244174c8a7d45741ffb22a25"),
    (0x0af8a8ff626c87a5, 0x5d146cc38b80296b, "9cdd2d38576c3f56519187f3de1a7a8fa6a9937f4ef7993f58727ed76c417d57"),
    (0x2bb3ea192be54d99, 0xf544d30749a46ff7, "a76c5b5de1931f8e0d3bc05e0d95dbe44d303bef72b6e43e58b0f2d0a5f40255"),
    (0x56f5c9194461d57c, 0xb47774feff7716a3, "342f017661b5a7608303918d0521b61fdb8540711ded9067d756674eb58d84dd"),
    (0x2a500531bdce91cc, 0xf8fa4dbd82a5df03, "8c52c1fbd083f2cbf6b34d1921a4c29f2361162bc16a3a4a626866d9db9f26f4"),
    (0xaba5451d524a8af9, 0xaba5451d524a8af9, "0222eefdfafb916b3ba05442283bda667dcb5df5ff00ee8c8aaa4770be5f1af4"),
    (0x680fb8260df3ed78, 0x680fb8260df3ed78, "b6827b62a8ae8583c67d7853a86fa7b8b95dc268280423d4148b40b0321429b8"),
    (0x860f559cd2cf9ffc, 0x860f559cd2cf9ffc, "c645224386e844aec7d267b6a851991c81d23c03d4de132a0a3ddc354e7d4b05"),
    (0x6f325e13d88cd4f1, 0x6f325e13d88cd4f1, "e7b10fcffa40b55f18c656a861773290efd9fc875c71b64fce61e0966b219a01"),
    (0x0edef548409e8fb7, 0x0edef548409e8fb7, "57ee10e7f8b99edfa0ff2bcc6a81fc1f04577f7186f555463827ea0a827a1242"),
    (0xeb0e1420a9fc2462, 0xc36446d156422b57, "c371f887db1f61b3d84f9cdeaf7ce3c30320fd3dadc1d7b7b8f2203447e767a2"),
    (0xdd362779f48a4b03, 0x8a810006f807ae2f, "d030bc3580ff989ed6ce83d4ac00133099fe4a2f6cd61d958d394468f4fe8074"),
    (0xd7bae7e66a6099a9, 0x6f1c2461a0979f67, "30988990f2df81d5265e83723723b96b724b0f4f3e00e54d6a3ccc4f9cdab331"),
    (0x0e4f6c4a3d0597ae, 0x0e4f6c4a3d0597ae, "343478cc1326d58c03e7ca5b4fe8f65eee7e6539762dd6cfe84b6a30fed3d2e2"),
    (0x2fffede2658aaf4f, 0x2fffede2658aaf4f, "fdc187d8d1496754e91e16c5f0ba9a051b9b783b88fc2acd926dfa109d36a79c"),
    (0x21b35a973325d7ca, 0x6ecbc8a57a8cba3f, "1e1de8a22eb07c82437189ffcd7e67d87625f0db70669f56873e54bf159e0a0d"),
    (0x56f5c9194461d57c, 0x8e3b9159fd70f0c1, "b3fbf1ca08475958e82efd7caea9000458710418046f2fcc142ceb6adc32a960"),
];

/// Word characters.
fn is_word(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'
}

/// The words of `text` (already lower-cased, `’` already `'`), with where
/// each starts and ends. `compound` keeps `- . , '` inside a word when a word
/// character follows; otherwise only `. ,` between digits are kept, and `_`
/// splits.
fn words(text: &[u8], compound: bool) -> Vec<(usize, usize)> {
    let inner = |b: u8| if compound { is_word(b) } else { b.is_ascii_lowercase() || b.is_ascii_digit() };
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if !inner(text[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < text.len() && inner(text[i]) {
            i += 1;
        }
        // Joiners, each only when a word character follows it.
        while i + 1 < text.len() && inner(text[i + 1]) {
            let j = text[i];
            let ok = if compound {
                matches!(j, b'-' | b'.' | b',' | b'\'')
            } else {
                matches!(j, b'.' | b',') && text[i - 1].is_ascii_digit() && text[i + 1].is_ascii_digit()
            };
            if !ok {
                break;
            }
            i += 1;
            while i < text.len() && inner(text[i]) {
                i += 1;
            }
        }
        out.push((start, i));
    }
    out
}

/// FNV-1a, 64 bits.
fn fnv(state: u64, bytes: &[u8]) -> u64 {
    let mut h = state;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
const FNV_START: u64 = 0xcbf29ce484222325;

/// Every fingerprint in `raw`, as the SHA-256 of the phrase found.
fn fingerprints_in(raw: &str, table: &[(u64, u64, &str)]) -> Vec<String> {
    let text: Vec<u8> = raw.to_lowercase().replace('\u{2019}', "'").into_bytes();
    let first: std::collections::HashSet<u64> = table.iter().map(|r| r.0).collect();
    let whole: std::collections::HashSet<u64> = table.iter().map(|r| r.1).collect();
    let mut found = Vec::new();
    for compound in [true, false] {
        let w = words(&text, compound);
        // Whether word i may continue a phrase from word i-1.
        let joins: Vec<bool> = (0..w.len())
            .map(|i| {
                i > 0
                    && text[w[i - 1].1..w[i].0]
                        .iter()
                        .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'*' | b'#' | b'>' | b'!' | b'-' | b'_'))
            })
            .collect();
        for i in 0..w.len() {
            let head = &text[w[i].0..w[i].1];
            let mut h = fnv(FNV_START, head);
            if !first.contains(&h) {
                continue;
            }
            let mut phrase = head.to_vec();
            let mut k = i;
            loop {
                if whole.contains(&h) {
                    let sha = atlas::digest::sha256_hex(&phrase);
                    if table.iter().any(|r| r.2 == sha) && !found.contains(&sha) {
                        found.push(sha);
                    }
                }
                k += 1;
                if k >= w.len() || k - i >= 4 || !joins[k] {
                    break;
                }
                let next = &text[w[k].0..w[k].1];
                h = fnv(fnv(h, b" "), next);
                phrase.push(b' ');
                phrase.extend_from_slice(next);
            }
        }
    }
    found
}

/// The repository root, when the crate sits in one.
fn repo_root() -> Option<std::path::PathBuf> {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let parent = crate_dir.parent()?;
    (parent.join(".git").exists() && parent.join("atlas").join("Cargo.toml").exists())
        .then(|| parent.to_path_buf())
}

/// Every file to look at: the crate, and the repository around it.
fn everything() -> Vec<std::path::PathBuf> {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    let mut files = Vec::new();
    walk(&crate_dir, &mut files);
    if let Some(root) = repo_root() {
        let Ok(entries) = std::fs::read_dir(&root) else { return files };
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if p == crate_dir || name == ".git" || name == "target" {
                continue;
            }
            if p.is_dir() {
                walk(&p, &mut files);
            } else {
                files.push(p);
            }
        }
    }
    files
}

#[test]
fn nothing_in_the_tree_carries_the_trading_systems_fingerprints() {
    let started = std::time::Instant::now();
    let base = repo_root().unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf());
    let files = everything();
    assert!(files.len() > 500, "the walk found only {} files", files.len());
    let mut carrying = Vec::new();
    let mut read = 0usize;
    for f in &files {
        let rel = f.strip_prefix(&base).unwrap_or(f).display().to_string();
        let mut found = fingerprints_in(&rel, FINGERPRINTS);
        if let Ok(bytes) = std::fs::read(f) {
            // Text only: a model file or a picture is not somewhere a person
            // wrote anything down, and its bytes match by chance.
            if !bytes[..bytes.len().min(8192)].contains(&0) {
                read += bytes.len();
                for sha in fingerprints_in(&String::from_utf8_lossy(&bytes), FINGERPRINTS) {
                    if !found.contains(&sha) {
                        found.push(sha);
                    }
                }
            }
        }
        if !found.is_empty() {
            let short: Vec<&str> = found.iter().map(|s| &s[..12]).collect();
            carrying.push(format!("{rel}  (fingerprint {})", short.join(", ")));
        }
    }
    assert!(
        carrying.is_empty(),
        "personal Atlas carries the trading system's own material in:\n  {}\n\n\
         Each fingerprint is the start of the SHA-256 of the phrase found; the plain \
         list lives with that system's own Atlas. That material belongs there, not in \
         the Atlas that is handed to friends.",
        carrying.join("\n  ")
    );
    assert!(read > 1_000_000, "only {read} bytes of text were read");
    // Kept fast on purpose: a guard people wait for is a guard people skip.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "the fingerprint scan took {:?}",
        started.elapsed()
    );
}

#[test]
fn the_fingerprint_scan_finds_a_phrase_however_it_is_written() {
    // The scanner, proved on a phrase that means nothing, so this test can
    // hold the phrase in plain text.
    let phrase = "zebra quilt lantern";
    let sha = atlas::digest::sha256_hex(phrase.as_bytes());
    let row = (fnv(FNV_START, b"zebra"), fnv(FNV_START, phrase.as_bytes()), sha.as_str());
    let table = [row];
    for text in [
        "a zebra quilt lantern here",
        "A ZEBRA Quilt LANTERN.",
        "// the zebra quilt\n// lantern, wrapped onto a second comment line",
        "* zebra\n  * quilt lantern",
        "file_zebra-quilt-lantern.md",
        "zebra_quilt_lantern",
    ] {
        assert_eq!(fingerprints_in(text, &table), vec![sha.clone()], "missed in {text:?}");
    }
    for text in ["zebra: {quilt} lantern", "zebra quilts lantern", "zebra, quilt lantern", "\"zebra\", \"quilt lantern\""] {
        assert!(fingerprints_in(text, &table).is_empty(), "false hit in {text:?}");
    }
    // And the real table is real: the one fingerprint this file may spell out
    // in pieces, the name, is in it.
    let name = the_name();
    for text in [name.clone(), name.to_uppercase(), format!("atlas-{name}"), format!("{name}_atlas")] {
        assert_eq!(fingerprints_in(&text, FINGERPRINTS).len(), 1, "the name is not caught in {text:?}");
    }
    assert!(FINGERPRINTS.len() >= 100, "the table has only {} rows", FINGERPRINTS.len());
}
