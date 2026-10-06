//! Fetching Atlas's voice pieces — from inside Atlas, not a batch file.
//!
//! This used to be `ATLAS.bat`'s `:setup`, and on 23 Sep 2026 checking it
//! against the real internet found it could not have worked:
//!
//! - **The listening engine's address was dead.** It asked for "the latest
//!   whisper.cpp release", and the latest release (v1.9.4) ships no Windows
//!   program at all — that address answered 404. The version is now pinned
//!   (v1.9.2, the newest that still ships `whisper-bin-x64.zip`).
//! - **The zip downloads were quoted wrong.** `'$env:TEMP\…'` in single quotes
//!   is never expanded by PowerShell, so whisper and piper would have been
//!   written to a folder literally named `$env:TEMP`, and failed.
//! - **Nothing was checked.** A half-finished or substituted download was
//!   accepted as long as a file existed.
//!
//! Every piece here is pinned to an exact file and its SHA-256, measured by
//! downloading it (23 Sep 2026). A download that doesn't match is thrown away
//! and said, never used. A piece already present is left alone, so running it
//! again picks up where it stopped.
//!
//! The fetching itself is Windows' own `curl.exe` and the unpacking Windows'
//! own `tar.exe` — both ship with Windows 10 and 11 — so Atlas carries no
//! HTTP or zip code of its own for this.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where a piece ends up.
#[derive(Debug, Clone, PartialEq)]
pub enum Lands {
    /// One file, at this install-relative path.
    File(&'static str),
    /// A zip: everything under `inside` (the folder in the zip) goes into
    /// `dir`; `key` is the file that proves it's there.
    Zip { inside: &'static str, dir: &'static str, key: &'static str },
}

/// One thing to fetch.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub name: &'static str,
    /// What Atlas can't do without it, in plain words.
    pub for_what: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub lands: Lands,
}

impl Piece {
    /// The file that says this piece is here.
    pub fn key_path(&self) -> &'static str {
        match &self.lands {
            Lands::File(p) => p,
            Lands::Zip { key, .. } => key,
        }
    }

    pub fn megabytes(&self) -> u64 {
        self.bytes.div_ceil(1_000_000)
    }
}

/// Where the sharper listening model lands, install-relative.
pub const SHARPER_LISTENING_MODEL: &str = "models/ggml-small.en-q5_1.bin";

/// Everything, in the order it's fetched — hearing first, because it unblocks
/// the most. Sizes and hashes measured 23 Sep 2026.
pub fn catalogue() -> Vec<Piece> {
    vec![
        Piece {
            name: "the listening engine",
            for_what: "hearing you",
            url: "https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.2/whisper-bin-x64.zip",
            sha256: "49dcc16de826f20bd53d44f947a1ae49dfa81f86cad67a64d80820cb192d674a",
            bytes: 8_194_445,
            lands: Lands::Zip { inside: "Release", dir: "tools/whisper", key: "tools/whisper/whisper-cli.exe" },
        },
        Piece {
            name: "the listening model",
            for_what: "hearing you",
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
            sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
            bytes: 147_964_211,
            lands: Lands::File("models/ggml-base.en.bin"),
        },
        // The sharper listening model (29 Sep 2026), preferred over the one
        // above whenever it is here (`language::speech_model_for`). On Eric's
        // laptop base.en heard "Atlas" as "At this" and "Brad", and "smart-
        // ass" as "smart apps". whisper.cpp's small.en at 5 bits: the next
        // size up (244M parameters to base's 74M), fewer word errors on
        // English, 190 MB. large-v3-turbo (574 MB at 5 bits) keeps
        // large-v3's full 32-layer encoder, which on the processor-only
        // whisper build setup ships is several times slower again for each
        // sentence -- too slow to wait for between turns. Not measured on
        // Eric's laptop yet: how long a sentence takes to be heard is in the
        // turn's timing line ("hearing="). The URL is the ggerganov/whisper.cpp
        // repository's own file; the SHA-256 is Hugging Face's LFS record of
        // it, and the file was downloaded and hashed to the same value on
        // 29 Sep 2026 (190,098,681 bytes).
        Piece {
            name: "the sharper listening model",
            for_what: "hearing you more accurately",
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en-q5_1.bin",
            sha256: "bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30",
            bytes: 190_098_681,
            lands: Lands::File(SHARPER_LISTENING_MODEL),
        },
        // Cutting in by voice (`micthread`, `barge_in`): Silero VAD's 16 kHz
        // opset-15 export, MIT. Round 3 made Atlas use it and setup never
        // fetched it, so the cut-in fell back to the plainer detector on
        // every install (28 Sep 2026). Pinned to the v6.2.3 tag of the
        // official repository (snakers4/silero-vad), downloaded and hashed
        // that day: the same bytes as tests/fixtures/silero, and the same
        // file master serves. Lands where `BargeInConfig::model` looks.
        Piece {
            name: "the voice cut-in model",
            for_what: "stopping when you talk over me",
            url: "https://raw.githubusercontent.com/snakers4/silero-vad/v6.2.3/src/silero_vad/data/silero_vad_16k_op15.onnx",
            sha256: "7ed98ddbad84ccac4cd0aeb3099049280713df825c610a8ed34543318f1b2c49",
            bytes: 1_289_603,
            lands: Lands::File("models/silero_vad_16k_op15.onnx"),
        },
        Piece {
            name: "the speaking engine",
            for_what: "talking back",
            url: "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip",
            sha256: "f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea",
            bytes: 22_477_236,
            lands: Lands::Zip { inside: "piper", dir: "tools/piper", key: "tools/piper/piper.exe" },
        },
        Piece {
            name: "a voice",
            for_what: "talking back",
            url: "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/amy/medium/en_US-amy-medium.onnx",
            sha256: "b3a6e47b57b8c7fbe6a0ce2518161a50f59a9cdd8a50835c02cb02bdd6206c18",
            bytes: 63_201_294,
            lands: Lands::File("models/en_US-amy-medium.onnx"),
        },
        Piece {
            name: "the voice's settings",
            for_what: "talking back",
            url: "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/amy/medium/en_US-amy-medium.onnx.json",
            sha256: "95a23eb4d42909d38df73bb9ac7f45f597dbfcde2d1bf9526fdeaf5466977d77",
            bytes: 4_882,
            lands: Lands::File("models/en_US-amy-medium.onnx.json"),
        },
        // 29 Sep 2026: moved from 7.1.1 to 9.0.2. An iPhone photo (HEIC) is
        // a grid of 512x512 tiles, and ffmpeg assembles the grid only from
        // 8.1 on: 7.1.1 handed back one tile, so every iPhone photo came in
        // as a 512-pixel corner of itself. 9.0.2 is gyan.dev's current
        // release (19 Sep 2026); this SHA-256 is the one gyan.dev publishes
        // beside its own copy (packages/ffmpeg-9.0.2-essentials_build.zip
        // .sha256), and the GitHub mirror's file was downloaded and hashed
        // to the same value on 29 Sep 2026. `tests/photo_editing.rs` decodes a
        // tiled HEIC and checks the whole picture comes back.
        Piece {
            name: "the sound tools",
            for_what: "recording and playing sound, and editing photos",
            url: "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip",
            sha256: "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba",
            bytes: 114_768_076,
            lands: Lands::Zip {
                inside: "ffmpeg-9.0.2-essentials_build/bin",
                dir: "tools/ffmpeg",
                key: "tools/ffmpeg/ffmpeg.exe",
            },
        },
    ]
}

/// Seeing: the eight OpenCV zoo models `vision`, `words` and the hands use,
/// about 133 MB. They used to be fetched by ATLAS.bat with no check at all;
/// these are the same files, pinned. Measured 24 Sep 2026, and each one run
/// on real pictures (`tests/seeing_real_pictures.rs`).
pub fn seeing() -> Vec<Piece> {
    vec![
        Piece {
            name: "finding faces",
            for_what: "seeing who's there",
            url: "https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx",
            sha256: "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
            bytes: 232_589,
            lands: Lands::File("models/face_detect.onnx"),
        },
        Piece {
            name: "telling faces apart",
            for_what: "knowing who's there",
            url: "https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/face_recognition_sface/face_recognition_sface_2021dec.onnx",
            sha256: "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79",
            bytes: 38_696_353,
            lands: Lands::File("models/face_id.onnx"),
        },
        Piece {
            name: "naming things",
            for_what: "naming what's in front of the camera",
            url: "https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/object_detection_yolox/object_detection_yolox_2022nov.onnx",
            sha256: "c5c2d13e59ae883e6af3b45daea64af4833a4951c92d116ec270d9ddbe998063",
            bytes: 35_858_002,
            lands: Lands::File("models/objects.onnx"),
        },
        Piece {
            name: "describing a picture",
            for_what: "saying what a picture is of",
            url: "https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/image_classification_mobilenet/image_classification_mobilenetv2_2022apr.onnx",
            sha256: "c0c3f76d93fa3fd6580652a45618618a220fced18babf65774ed169de0432ad5",
            bytes: 13_964_571,
            lands: Lands::File("models/picture.onnx"),
        },
        Piece {
            name: "finding your hands",
            for_what: "hand gestures",
            url: "https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/palm_detection_mediapipe/palm_detection_mediapipe_2023feb.onnx",
            sha256: "78ff51c38496b7fc8b8ebdb6cc8c1abb02fa6c38427c6848254cdaba57fcce7c",
            bytes: 3_905_734,
            lands: Lands::File("models/hand_presence.onnx"),
        },
        Piece {
            name: "reading your fingers",
            for_what: "hand gestures",
            url: "https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/handpose_estimation_mediapipe/handpose_estimation_mediapipe_2023feb.onnx",
            sha256: "db0898ae717b76b075d9bf563af315b29562e11f8df5027a1ef07b02bef6d81c",
            bytes: 4_099_621,
            lands: Lands::File("models/hand_landmarks.onnx"),
        },
        Piece {
            name: "finding words on the screen",
            for_what: "reading your screen",
            url: "https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/text_detection_ppocr/text_detection_en_ppocrv3_2023may.onnx",
            sha256: "03f550c6b406fda8bf54bd8327815f6c7e2edd98cea02348c93d879254366587",
            bytes: 2_423_490,
            lands: Lands::File("models/text_find.onnx"),
        },
        Piece {
            name: "reading those words",
            for_what: "reading your screen",
            url: "https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/text_recognition_crnn/text_recognition_CRNN_EN_2021sep.onnx",
            sha256: "a84b1f6e11a65c2d733cb0cc1f014aae3f99051e3f11447dc282faa678eee544",
            bytes: 33_823_087,
            lands: Lands::File("models/text_read.onnx"),
        },
    ]
}

/// Reading pictures — charts, screenshots, photos — with a local model that
/// can talk about them (`picture_talk`). About 3 GB, so it is its own
/// download rather than part of setting up. llama.cpp's Vulkan build, which
/// uses the laptop's own graphics and falls back to the processor; Qwen3-VL
/// 4B Instruct at 4-bit, and its picture encoder, both Qwen's own files.
pub fn pictures() -> Vec<Piece> {
    vec![
        Piece {
            name: "the thinking engine",
            for_what: "answering you, and reading screens",
            url: "https://github.com/ggml-org/llama.cpp/releases/download/b10456/llama-b10456-bin-win-vulkan-x64.zip",
            sha256: "60f3d31cc7c2fe62de8f34f8d75ffd06655b4de83bcc5aa6f08df56be42ebb91",
            // The real size of that file (28 Sep 2026: this said 33_200_000,
            // an estimate, so progress stopped short and the space check was
            // off). The hash above is what decides.
            bytes: 34_807_256,
            lands: Lands::Zip { inside: "", dir: "tools/llama", key: "tools/llama/llama-mtmd-cli.exe" },
        },
        faster_talk_model(),
        Piece {
            name: "its picture reader",
            for_what: "reading screens and pictures",
            url: "https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/resolve/main/mmproj-Qwen3VL-4B-Instruct-Q8_0.gguf",
            sha256: "30ba2c7dd3127a4561b6cba9d13d0f711c91bdb38742e2f56d73c8cb596bd06d",
            bytes: 453_974_304,
            lands: Lands::File("models/mmproj-Qwen3VL-4B-Instruct-Q8_0.gguf"),
        },
    ]
}

/// The helper model for faster replies (`models.draft`): Qwen3-0.6B at 8-bit,
/// Qwen's own file. Checked on 28 Sep 2026 against Hugging Face's record of
/// it (the LFS sha256 and size), and its vocabulary read out of both files'
/// headers: 151,936 tokens, every one the same as the shipped Qwen3-VL 4B's,
/// same beginning and end tokens -- which is what llama.cpp's
/// `common_speculative_are_compatible` demands, or the server won't start.
/// Not part of setup: fetched only when asked for on the Connections page.
pub fn draft_model() -> Piece {
    Piece {
        name: "the helper model",
        for_what: "answering a little faster",
        url: "https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/main/Qwen3-0.6B-Q8_0.gguf",
        sha256: "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031",
        bytes: 639_446_688,
        lands: Lands::File("models/Qwen3-0.6B-Q8_0.gguf"),
    }
}

/// "Better answers" (`models.talk: better`, `deepbrain`): Qwen3.5 4B at
/// Q4_K_M, bartowski's quantisation of Qwen's own weights, pinned to that
/// repository's commit. Size and SHA-256 are Hugging Face's LFS record of
/// the file (30 Sep 2026), and the same as Eric's copy, hashed on his laptop.
/// Its picture encoder (`mmproj-Qwen_Qwen3.5-4B-f16.gguf`, 672 MB) is not
/// fetched: pictures stay with the Qwen3-VL model (`picture_talk`), because
/// Qwen3.5's own template thinks out loud unless told not to, and the
/// picture program isn't told.
pub fn better_talk_model() -> Piece {
    Piece {
        name: "the better model",
        for_what: "more natural answers",
        url: "https://huggingface.co/bartowski/Qwen_Qwen3.5-4B-GGUF/resolve/4168f45a16a1290d65a4ec0fa312ae917a4c15d6/Qwen_Qwen3.5-4B-Q4_K_M.gguf",
        sha256: "13c16f426047e2de38cd075bdade4a7bcbc8c774384876f677740cda65f8a983",
        bytes: 3_013_027_808,
        lands: Lands::File("models/Qwen_Qwen3.5-4B-Q4_K_M.gguf"),
    }
}

/// The bigger talking model (`deepbrain::BIGGER_TALK`) and its picture
/// encoder: Qwen3-VL 8B Instruct, Qwen's own GGUF files (Apache-2.0), pinned
/// to the repository's commit. Sizes and SHA-256 are Hugging Face's LFS
/// record of each file, read on 2 Oct 2026 from the repository's file list
/// at that commit and from the download's own `X-Linked-ETag` -- the same
/// record that matches the shipped 4B's pinned hashes exactly. Not yet
/// downloaded and hashed on a machine of ours; `fetch` checks the hash on
/// arrival either way. Only offered where the machine has room
/// (`models::room_for_bigger_talk`).
pub fn bigger_talk_model() -> Vec<Piece> {
    vec![
        Piece {
            name: "the bigger model",
            for_what: "understanding you better, on a machine with room",
            url: "https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF/resolve/f982a07559d4a2f6c8744d840bf6fccab30eea96/Qwen3VL-8B-Instruct-Q4_K_M.gguf",
            sha256: "67d1659bfe71b89d50b45a4ad1a9e5b997e5bb16ce5da66a6a6167abd569e9e2",
            bytes: 5_027_784_800,
            lands: Lands::File("models/Qwen3VL-8B-Instruct-Q4_K_M.gguf"),
        },
        Piece {
            name: "the bigger model's picture reader",
            for_what: "reading screens and pictures with the bigger model",
            url: "https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF/resolve/f982a07559d4a2f6c8744d840bf6fccab30eea96/mmproj-Qwen3VL-8B-Instruct-Q8_0.gguf",
            sha256: "c6ba85508d82f42590e6eb77d5340369ab6fecf107a7561d809523d8aa5f3bfd",
            bytes: 752_289_728,
            lands: Lands::File("models/mmproj-Qwen3VL-8B-Instruct-Q8_0.gguf"),
        },
    ]
}

/// About how much memory the helpers installed here hold while Atlas runs,
/// in MB: hearing (Parakeet), the wake-word spotter, telling voices apart,
/// the speaking voice, the meaning model and the seeing models (hands
/// included). Estimated from their files -- half again on top for what they
/// take running -- not measured: enough to keep a bigger talking model from
/// being chosen into memory they already use.
pub fn helpers_resident_mb(root: &Path) -> u64 {
    let bytes: u64 = [parakeet_pieces(), vec![crate::kws::spotter_piece()], voice_model(), crate::kokoro::pieces(), understanding(), seeing()]
        .into_iter()
        .flatten()
        .filter(|p| have(p, root))
        .map(|p| p.bytes)
        .sum();
    bytes * 3 / 2 / (1024 * 1024)
}

/// The shipped talking model, as `pictures` fetches it (one definition, used
/// by both: it was found in `pictures()` by name with an `expect`, audit Q2).
pub fn faster_talk_model() -> Piece {
    Piece {
        name: "the language model",
        for_what: "answering you, and reading screens",
        url: "https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/resolve/main/Qwen3VL-4B-Instruct-Q4_K_M.gguf",
        sha256: "66358cb18bb6b3b1b6675aa412c7a88ef01d228f481184d13668e5201c730a0a",
        bytes: 2_497_281_664,
        lands: Lands::File("models/Qwen3VL-4B-Instruct-Q4_K_M.gguf"),
    }
}

/// The deep brain (`models.deep`, `deepbrain`): Qwen3.5 9B at IQ4_XS,
/// bartowski's, pinned to that repository's commit. Size and SHA-256 as for
/// `better_talk_model` (Hugging Face's LFS record, 30 Sep 2026; the same as
/// Eric's copy).
pub fn deep_model() -> Piece {
    Piece {
        name: "the deep brain",
        for_what: "research, drafts and summaries written with more care",
        url: "https://huggingface.co/bartowski/Qwen_Qwen3.5-9B-GGUF/resolve/182be2fd6c7bc44887d88a91cb03ff009cc9f549/Qwen_Qwen3.5-9B-IQ4_XS.gguf",
        sha256: "7d977cc96c2e08616016d967f232083e354691a8a16f345b26f7d782ee5c9601",
        bytes: 5_501_202_464,
        lands: Lands::File("models/Qwen_Qwen3.5-9B-IQ4_XS.gguf"),
    }
}

/// The coding model (`coder`, 2 Oct 2026): Qwen2.5-Coder 7B Instruct at
/// Q4_K_M, Qwen's own single-file GGUF, pinned to that repository's commit
/// (13fb94bf). Licence Apache-2.0 (the repository's own card). Size and
/// SHA-256 are Hugging Face's LFS record of the file, and the same as the
/// file itself, downloaded and hashed on 2 Oct 2026. Fetched on a machine
/// with room for it (`coder::size_for`), not on every one.
pub fn coder_model() -> Piece {
    Piece {
        name: "the coding model",
        for_what: "writing and fixing code",
        url: "https://huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF/resolve/13fb94bfda8c8cf22497dc57b78f391a9acb426a/qwen2.5-coder-7b-instruct-q4_k_m.gguf",
        sha256: "509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c",
        bytes: 4_683_073_536,
        lands: Lands::File("models/qwen2.5-coder-7b-instruct-q4_k_m.gguf"),
    }
}

/// The coding model for a small machine: Qwen2.5-Coder 1.5B Instruct at
/// Q4_K_M, Qwen's own, pinned to commit f86cb2c1, Apache-2.0; checked the
/// same way as `coder_model` (2 Oct 2026). The 3B between them is not
/// offered: Qwen published it under its research licence, non-commercial
/// only.
pub fn small_coder_model() -> Piece {
    Piece {
        name: "the small coding model",
        for_what: "writing and fixing code",
        url: "https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF/resolve/f86cb2c1fa58255f8052cc32aeede1b7482d4361/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf",
        sha256: "cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046",
        bytes: 1_117_320_768,
        lands: Lands::File("models/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf"),
    }
}

/// A size as a button says it: "5.1 GB" (binary gigabytes, as Windows shows
/// a file's size), or megabytes under one.
pub fn gib_label(bytes: u64) -> String {
    let gib = bytes as f64 / (1u64 << 30) as f64;
    if gib >= 1.0 {
        format!("{gib:.1} GB")
    } else {
        format!("{} MB", (bytes + (1 << 20) - 1) >> 20)
    }
}

/// Where a model may already be on this computer besides the models folder:
/// a `model-bench` folder in Atlas's folder or beside it (where Eric
/// measured them, 30 Sep 2026).
pub fn places_it_may_be(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.join("model-bench")];
    if let Some(up) = root.parent() {
        out.push(up.join("model-bench"));
    }
    out
}

/// A one-file piece already on this computer in one of `places`, with the
/// right size and SHA-256: moved into place (copied, where it can't be
/// moved). The folder it came from, or `None` when it isn't anywhere.
pub fn take_in(p: &Piece, root: &Path, places: &[PathBuf]) -> Result<Option<PathBuf>, String> {
    let Lands::File(rel) = &p.lands else { return Ok(None) };
    let Some(file) = Path::new(rel).file_name() else { return Ok(None) };
    let dest = root.join(rel);
    for place in places {
        let found = place.join(file);
        if found == dest || std::fs::metadata(&found).map(|m| m.len()).ok() != Some(p.bytes) {
            continue;
        }
        let got = crate::digest::sha256_file_hex(&found).map_err(|e| format!("I couldn't read {}: {e}", found.display()))?;
        if !got.eq_ignore_ascii_case(p.sha256) {
            continue;
        }
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&found, &dest)
            .or_else(|_| {
                let beside = dest.with_extension("incoming");
                std::fs::copy(&found, &beside).and_then(|_| std::fs::rename(&beside, &dest)).inspect_err(|_| {
                    crate::heard!(std::fs::remove_file(&beside));
                })
            })
            .map_err(|e| format!("I couldn't put {} in place: {e}", p.name))?;
        return Ok(Some(place.clone()));
    }
    Ok(None)
}

/// Tor, for reaching friends from anywhere (`onion`): the Tor Project's own
/// "expert bundle" -- `tor` and the pluggable transports (`lyrebird`, for the
/// bridges a network that blocks Tor needs, with their `pt_config.json`).
///
/// Pinned to 15.0.23 (tor 0.4.9.12). Both bundles were checked on 27 Sep 2026
/// against the Tor Browser Developers signing key (EF6E 286D DA85 EA2A 4BA7
/// DE68 4E2C 6E87 9329 8290) and against `sha256sums-signed-build.txt`; the
/// SHA-256 here is that checked file's. It lands in `tor/` inside the install,
/// which is where `onion::find_tor` looks (`tor/tor.exe` beside Atlas).
///
/// The Tor Project removes old versions from `dist.torproject.org` a few weeks
/// after the next release, so this address goes stale by design:
/// `tests/tor_ships_with_atlas.rs` says so and names the page to re-pin from.
/// The archive keeps every version (`archive.torproject.org`), which is the
/// address used here for exactly that reason.
pub fn tor() -> Vec<Piece> {
    if cfg!(windows) {
        vec![Piece {
            name: "Tor",
            for_what: "friends reaching your Atlas from anywhere",
            url: "https://archive.torproject.org/tor-package-archive/torbrowser/15.0.23/tor-expert-bundle-windows-x86_64-15.0.23.tar.gz",
            sha256: "231dad6b9cb401a54c260db7046965ef04e4f72ff071b140d423fb5da281ab1e",
            bytes: 22_432_027,
            lands: Lands::Zip { inside: "tor", dir: "tor", key: "tor/tor.exe" },
        }]
    } else {
        vec![Piece {
            name: "Tor",
            for_what: "friends reaching your Atlas from anywhere",
            url: "https://archive.torproject.org/tor-package-archive/torbrowser/15.0.23/tor-expert-bundle-linux-x86_64-15.0.23.tar.gz",
            sha256: "08d49de27f542b8f73e2014e064d8320562b5d20019c03d4725c5a5249d97985",
            bytes: 32_339_495,
            lands: Lands::Zip { inside: "tor", dir: "tor", key: "tor/tor" },
        }]
    }
}

/// Cutting out a photo's subject (`cutout`): blurring or removing the
/// background. Optional -- nothing else needs them, and the photo editing
/// works without. Both Apache-2.0, code and weights. Hashes and sizes from
/// Hugging Face's record and the rembg release, checked by downloading each
/// on 29 Sep 2026, and each loaded and run in tract (`tests/photo_editing.rs`).
///
/// - MODNet, portrait matting (github.com/ZHKKKe/MODNet), as exported to
///   ONNX by Xenova, pinned to the commit rather than `main`.
/// - u2netp, the small U^2-Net (github.com/xuebinqin/U-2-Net), the file
///   rembg (MIT) publishes and checks by MD5 8e83ca70e441ab06c318d82300c84806.
///
/// Not RMBG: BRIA's licence is non-commercial.
pub fn photos() -> Vec<Piece> {
    vec![
        Piece {
            name: "the portrait cut-out model",
            for_what: "blurring or removing the background behind people",
            url: "https://huggingface.co/Xenova/modnet/resolve/fa2fa546052fba4c08921230a26cc69a333fca12/onnx/model.onnx",
            sha256: "07c308cf0fc7e6e8b2065a12ed7fc07e1de8febb7dc7839d7b7f15dd66584df9",
            bytes: 25_888_640,
            lands: Lands::File("models/modnet.onnx"),
        },
        Piece {
            name: "the general cut-out model",
            for_what: "blurring or removing the background behind anything else",
            url: "https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2netp.onnx",
            sha256: "309c8469258dda742793dce0ebea8e6dd393174f89934733ecc8b14c76f4ddd8",
            bytes: 4_574_861,
            lands: Lands::File("models/u2netp.onnx"),
        },
    ]
}

/// What lets Atlas understand what you mean, not only the words you used
/// (30 Sep 2026): the meaning model, run inside Atlas (`meaningnative`), and
/// its word list. Both Hugging Face's own files from the model's page,
/// hash-checked; 90 MB together. Tools are then chosen by meaning as well as
/// words, and search by meaning needs no separate program.
pub fn understanding() -> Vec<Piece> {
    vec![
        Piece {
            name: "the meaning model",
            for_what: "understanding what you mean, not only the words you used",
            url: "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx",
            sha256: "6fd5d72fe4589f189f8ebc006442dbb529bb7ce38f8082112682524616046452",
            bytes: 90_405_214,
            lands: Lands::File("models/understanding/all-MiniLM-L6-v2.onnx"),
        },
        Piece {
            name: "its word list",
            for_what: "understanding what you mean, not only the words you used",
            url: "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/vocab.txt",
            sha256: "07eced375cec144d27c900241f3e339478dec958f92fddbc551f295c992038a3",
            bytes: 231_508,
            lands: Lands::File("models/understanding/vocab.txt"),
        },
    ]
}

/// The picture maker (`imagemake`, 30 Sep 2026): stable-diffusion.cpp's
/// Vulkan build for Windows (release master-890-74988b2, MIT), Z-Image Turbo
/// at 4-bit (Apache 2.0), its Qwen3 4B text encoder (Apache 2.0) and its
/// decoder. Sizes and SHA-256 from Hugging Face's own records and the
/// release file, 30 Sep 2026. About 6.5 GB.
pub fn picture_making() -> Vec<Piece> {
    vec![
        Piece {
            name: "the picture maker",
            for_what: "making pictures on this machine",
            url: "https://github.com/leejet/stable-diffusion.cpp/releases/download/master-890-74988b2/sd-master-74988b2-bin-win-vulkan-x64.zip",
            sha256: "744c8f817c66ecfd02fbb9dc8b122e1f29f7240db1f6086dfde2669403c5d896",
            bytes: 31_932_748,
            lands: Lands::Zip { inside: "", dir: "tools/sd", key: "tools/sd/sd-cli.exe" },
        },
        Piece {
            name: "its picture model",
            for_what: "making pictures on this machine",
            url: "https://huggingface.co/leejet/Z-Image-Turbo-GGUF/resolve/main/z_image_turbo-Q4_0.gguf",
            sha256: "2bc57986874c84f7ec6d02d9d7070a53b0029954a0e38a6e1342eb91095572f5",
            bytes: 3_683_370_944,
            lands: Lands::File("models/pictures/z_image_turbo-Q4_0.gguf"),
        },
        Piece {
            name: "its text encoder",
            for_what: "making pictures on this machine",
            url: "https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF/resolve/main/Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
            sha256: "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597",
            bytes: 2_497_281_120,
            lands: Lands::File("models/pictures/Qwen3-4B-Instruct-2507-Q4_K_M.gguf"),
        },
        Piece {
            name: "its decoder",
            for_what: "making pictures on this machine",
            url: "https://huggingface.co/Comfy-Org/z_image_turbo/resolve/main/split_files/vae/ae.safetensors",
            sha256: "afc8e28272cd15db3919bacdb6918ce9c1ed22e96cb12c4d5ed0fba823529e38",
            bytes: 335_304_388,
            lands: Lands::File("models/pictures/ae.safetensors"),
        },
    ]
}

/// Telling your voice from others' (`speakernet`): 3D-Speaker's CAM++
/// English VoxCeleb model (Apache-2.0) as sherpa-onnx publishes it. Hash and
/// size from downloading it on 30 Sep 2026; measured against onnxruntime in
/// `tests/voices_told_apart.rs`.
pub fn voice_model() -> Vec<Piece> {
    vec![Piece {
        name: "the voice model",
        for_what: "telling your voice from other voices, far more reliably",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
        sha256: "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b",
        bytes: 29_596_978,
        lands: Lands::File("models/campplus_en_voxceleb.onnx"),
    }]
}

/// Better hearing (`parakeet`): sherpa-onnx 1.13.8's server and NVIDIA's
/// Parakeet TDT 0.6B v2 (int8). Hashes and sizes from downloading each on
/// 30 Sep 2026; the model and the Linux build were run through Atlas's own
/// client on LibriSpeech speech (`parakeet.rs` has the numbers). sherpa-onnx
/// is Apache-2.0; Parakeet TDT 0.6B v2 is CC-BY-4.0.
fn parakeet_pieces() -> Vec<Piece> {
    let server = if cfg!(windows) {
        Piece {
            name: "the hearing server",
            for_what: "hearing you with Parakeet, which mishears far less than whisper",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-no-tts.tar.bz2",
            sha256: "4b0a94f7b5c606b1b64a19a831c2127559e4b3d34e195465ebc7be73d9ed4783",
            bytes: 23_271_851,
            lands: Lands::Zip {
                inside: "sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-no-tts",
                dir: "tools/sherpa",
                key: "tools/sherpa/bin/sherpa-onnx-offline-websocket-server.exe",
            },
        }
    } else {
        Piece {
            name: "the hearing server",
            for_what: "hearing you with Parakeet, which mishears far less than whisper",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-linux-x64-shared-no-tts.tar.bz2",
            sha256: "d0f96c8b65c6cd0974fada22737e337de81bc8cd2abbec2e39caf358b1eec5fc",
            bytes: 24_802_494,
            lands: Lands::Zip {
                inside: "sherpa-onnx-v1.13.8-linux-x64-shared-no-tts",
                dir: "tools/sherpa",
                key: "tools/sherpa/bin/sherpa-onnx-offline-websocket-server",
            },
        }
    };
    vec![
        server,
        Piece {
            name: "the Parakeet hearing model",
            for_what: "hearing you across the room, not just up close",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2",
            sha256: "157c157bc51155e03e37d2466522a3a737dd9c72bb25f36eb18912964161e1ad",
            bytes: 482_468_385,
            lands: Lands::Zip {
                inside: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8",
                dir: "models/parakeet",
                key: "models/parakeet/encoder.int8.onnx",
            },
        },
    ]
}

/// A set by the word you'd type after `atlas get`: nothing for the voice
/// pieces, `seeing`, `pictures`, `photos`, or `tor`.
pub fn set(word: Option<&str>) -> Option<(&'static str, Vec<Piece>)> {
    match word.map(|w| w.trim().to_lowercase()).as_deref() {
        None | Some("") | Some("voice") => Some(("what Atlas needs to hear you and talk back", catalogue())),
        Some("seeing") => Some(("what Atlas needs to see faces, things, hands and the words on your screen", seeing())),
        Some("pictures") => Some(("what Atlas needs to read charts and screens (about 3 GB)", pictures())),
        Some("tor" | "friends") => Some(("Tor, so friends can reach your Atlas from anywhere", tor())),
        Some("photos" | "photo") => Some(("the cut-out models Atlas needs to blur or remove a photo's background", photos())),
        Some("pictures-made" | "picture-maker" | "imagemake") => Some(("what Atlas needs to make pictures on this machine (about 6.5 GB)", picture_making())),
        // Everything a model or a program for this machine, in one go (30
        // Sep 2026: "make the picture and meaning model download on
        // install"). Tor isn't one: it's for reaching friends, not for
        // anything Atlas does here.
        Some("everything" | "all") => {
            let mut all = catalogue();
            for more in [understanding(), crate::kokoro::pieces(), seeing(), photos(), pictures(), picture_making(), parakeet_pieces(), vec![crate::kws::spotter_piece()]] {
                for p in more {
                    if !all.iter().any(|a: &Piece| a.key_path() == p.key_path()) {
                        all.push(p);
                    }
                }
            }
            Some(("everything Atlas can use on this machine: voice, Parakeet hearing, understanding, seeing, reading and making pictures (about 11.5 GB)", all))
        }
        Some("understanding" | "meaning") => Some(("what Atlas needs to understand what you mean, not only your words (90 MB)", understanding())),
        Some("hearing" | "parakeet") => Some(("Parakeet, so Atlas hears you better, and the wake-word spotter (about 520 MB)", [parakeet_pieces(), vec![crate::kws::spotter_piece()]].concat())),
        Some("voiceid" | "voice-id" | "voices") => Some(("the voice model, so Atlas can tell your voice from others (30 MB)", voice_model())),
        Some("wakeword" | "wake-word" | "wake" | "kws") => Some(("the wake-word spotter, so Atlas hears its name by the sound (18 MB)", vec![crate::kws::spotter_piece()])),
        Some("kokoro") => Some(("the Kokoro voice, which sounds much more natural than piper", crate::kokoro::pieces())),
        // The coding model this machine has room for (2 Oct 2026, `coder`):
        // the 7B, the 1.5B on a small machine, nothing on a very small one.
        Some("coder" | "coding" | "code") => Some(("the coding model this computer has room for, so Atlas writes code with a model trained for it", crate::coder::pieces_for_here())),
        _ => None,
    }
}

/// Is it here already?
///
/// A zip is judged by the mark `fetch` writes once *everything* in it has been
/// copied into place (`marker_path`), holding the zip's SHA-256. Until 28 Sep
/// 2026 it was judged by its key file alone, so a copy that failed partway
/// (a full disk, a file held open) after the key file landed counted as
/// installed for ever. An install from before the mark existed is accepted
/// once, and marked, if its key file is there and nothing of an unfinished
/// fetch is left beside it (`fetch` removes its download and unpacked copy
/// only after the whole copy succeeded).
pub fn have(p: &Piece, root: &Path) -> bool {
    let key = root.join(p.key_path());
    match &p.lands {
        // A lone file is judged by its size: the whole point of the pin is
        // that the right file has exactly this many bytes.
        Lands::File(_) => std::fs::metadata(&key).map(|m| m.len() == p.bytes).unwrap_or(false),
        Lands::Zip { .. } => {
            let Some(mark) = marker_path(p, root) else { return false };
            if let Ok(text) = std::fs::read_to_string(&mark) {
                return text.trim().eq_ignore_ascii_case(p.sha256) && key.is_file();
            }
            // Before the mark: the key file, whole, and no unfinished fetch.
            let work = work_dir(root);
            let unfinished = work.join(format!("{}.part", slug(p.name))).exists()
                || work.join(format!("{}.unpacked", slug(p.name))).exists();
            let key_ok = std::fs::metadata(&key).is_ok_and(|m| m.is_file() && m.len() > 0);
            if key_ok && !unfinished {
                crate::kept!(std::fs::write(&mark, p.sha256));
                return true;
            }
            false
        }
    }
}

/// Where a zip piece's "all of it is here" mark lives: in its own folder.
pub fn marker_path(p: &Piece, root: &Path) -> Option<PathBuf> {
    match &p.lands {
        Lands::Zip { dir, .. } => Some(root.join(dir).join(MARKER)),
        Lands::File(_) => None,
    }
}

/// The mark's file name (see `have`).
pub const MARKER: &str = ".atlas-piece";

fn work_dir(root: &Path) -> PathBuf {
    root.join("data").join("tmp").join("downloads")
}

// ---------------------------------------------------------------- room on the disk

/// Room kept free beyond what the pieces need, so the setup never fills the
/// disk to the last byte.
pub const SPARE_BYTES: u64 = 500_000_000;

/// What fetching `pieces` into `root` still needs on disk: each missing
/// piece's size, less what's already downloaded of it, twice over (a zip is
/// downloaded and then unpacked beside it), plus `SPARE_BYTES`. Zero when
/// nothing is missing.
pub fn space_needed(pieces: &[Piece], root: &Path) -> u64 {
    let work = work_dir(root);
    let missing: u64 = pieces
        .iter()
        .filter(|p| !have(p, root))
        .map(|p| {
            let part = std::fs::metadata(work.join(format!("{}.part", slug(p.name)))).map(|m| m.len()).unwrap_or(0);
            p.bytes.saturating_sub(part)
        })
        .sum();
    if missing == 0 {
        0
    } else {
        missing.saturating_mul(2).saturating_add(SPARE_BYTES)
    }
}

/// Is there room? `free` is what the disk holding `root` has (`None`:
/// couldn't tell, which is not a reason to stop). `Err` says how much is
/// needed, how much there is, and where.
pub fn room_for(pieces: &[Piece], root: &Path, free: Option<u64>) -> Result<(), String> {
    let needed = space_needed(pieces, root);
    let Some(free) = free else { return Ok(()) };
    if needed == 0 || free >= needed {
        return Ok(());
    }
    let gb = |b: u64| format!("{:.1} GB", b as f64 / 1_000_000_000.0);
    Err(format!(
        "There isn't room to finish setting up: Atlas needs about {} free on the drive holding {}, and it has {}. \
         Free up {} there (empty the Recycle Bin, or move some large files), then open Atlas again -- it picks up \
         where it stopped.",
        gb(needed),
        root.display(),
        gb(free),
        gb(needed - free)
    ))
}

/// Bytes free on the disk holding `path`, if the system says.
pub fn free_bytes(path: &Path) -> Option<u64> {
    // The folder itself may not exist yet: ask about the nearest that does.
    let mut at = path.to_path_buf();
    while !at.exists() {
        at = at.parent()?.to_path_buf();
    }
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let mut free = 0u64;
        // SAFETY: the string outlives the call; only the out value is written.
        unsafe { GetDiskFreeSpaceExW(&HSTRING::from(at.as_os_str()), Some(&mut free), None, None) }.ok()?;
        Some(free)
    }
    #[cfg(not(windows))]
    {
        // `df -P -k`: POSIX output, 1024-byte blocks; the 4th column is what's
        // available to this user.
        let out = crate::tools::command("df").arg("-P").arg("-k").arg(&at).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().nth(1)?;
        line.split_whitespace().nth(3)?.parse::<u64>().ok().map(|k| k * 1024)
    }
}

/// Remove the unfinished downloads and unpacked copies of `pieces` -- what a
/// fetch that failed for want of room leaves, which only makes the room
/// shorter. Returns how many bytes that gave back.
pub fn clear_unfinished(pieces: &[Piece], root: &Path) -> u64 {
    let work = work_dir(root);
    let mut freed = 0;
    for p in pieces {
        let part = work.join(format!("{}.part", slug(p.name)));
        freed += std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        crate::heard!(std::fs::remove_file(&part));
        crate::heard!(std::fs::remove_dir_all(work.join(format!("{}.unpacked", slug(p.name)))));
    }
    freed
}

/// How the fetching and unpacking are done. Windows' own tools by default;
/// swappable so the tests can run the same code on any machine.
#[derive(Debug, Clone)]
pub struct Tools {
    pub curl: String,
    /// `tar` on Windows (bsdtar reads zips); `unzip` elsewhere.
    pub unzip: Unzip,
    /// The unpacking program itself.
    pub unzipper: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unzip {
    Tar,
    Unzip,
}

impl Default for Tools {
    fn default() -> Self {
        if cfg!(windows) {
            // Windows' own copies by full path: a `tar` from Git for Windows
            // earlier on PATH is GNU tar, which can't read a zip.
            let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
            let own = |name: &str| {
                let p = PathBuf::from(&sys).join("System32").join(name);
                if p.is_file() { p.display().to_string() } else { name.to_string() }
            };
            Tools { curl: own("curl.exe"), unzip: Unzip::Tar, unzipper: own("tar.exe") }
        } else {
            Tools { curl: "curl".into(), unzip: Unzip::Unzip, unzipper: "unzip".into() }
        }
    }
}

/// Fetch one piece into the install at `root`, calling `progress(done, total)`
/// as bytes arrive. Resumes a download that was cut off.
pub fn fetch(p: &Piece, root: &Path, tools: &Tools, progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    if have(p, root) {
        progress(p.bytes, p.bytes);
        return Ok(());
    }
    let work = work_dir(root);
    std::fs::create_dir_all(&work).map_err(|e| format!("I couldn't make a place to download into: {e}"))?;
    let part = work.join(format!("{}.part", slug(p.name)));

    if let Err(why) = download(p, &part, tools, progress) {
        // A download that failed for want of room is thrown away: what's
        // there can't be finished, and only makes the disk fuller.
        if why.contains("disk") {
            crate::heard!(std::fs::remove_file(&part));
        }
        return Err(why);
    }

    // Checked before anything is unpacked or moved into place.
    let got = crate::digest::sha256_file_hex(&part).map_err(|e| format!("I couldn't read what I downloaded: {e}"))?;
    if !got.eq_ignore_ascii_case(p.sha256) {
        crate::heard!(std::fs::remove_file(&part));
        return Err(format!(
            "what arrived for {} isn't the file it should be, so I threw it away. Try again; if it \
             keeps happening, the file has changed where it's kept",
            p.name
        ));
    }

    match &p.lands {
        Lands::File(rel) => {
            let dest = root.join(rel);
            if let Some(d) = dest.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            // Where it can't simply be moved, copied beside it and then moved
            // over it, so the file there is never half written.
            std::fs::rename(&part, &dest)
                .or_else(|_| {
                    let beside = dest.with_extension("incoming");
                    std::fs::copy(&part, &beside)
                        .and_then(|_| std::fs::rename(&beside, &dest))
                        .and_then(|_| std::fs::remove_file(&part))
                        .inspect_err(|_| {
                            crate::heard!(std::fs::remove_file(&beside));
                        })
                })
                .map_err(|e| format!("I couldn't put {} in place: {e}", p.name))?;
        }
        Lands::Zip { inside, dir, .. } => {
            let unpack = work.join(format!("{}.unpacked", slug(p.name)));
            crate::heard!(std::fs::remove_dir_all(&unpack));
            std::fs::create_dir_all(&unpack).map_err(|e| e.to_string())?;
            if let Err(why) = unzip(&part, &unpack, tools) {
                crate::heard!(std::fs::remove_dir_all(&unpack));
                return Err(why);
            }
            let from = unpack.join(inside);
            if !from.is_dir() {
                crate::heard!(std::fs::remove_dir_all(&unpack));
                return Err(format!("{} didn't unpack the way it should have", p.name));
            }
            // Swapped in whole, not copied over the one there (29 Sep 2026:
            // a copy that stopped partway -- a program in use, a full disk --
            // left the tool half old, half new, and broken).
            let mark = root.join(dir).join(MARKER);
            if let Err(e) = swap_folder(&from, &root.join(dir)) {
                crate::heard!(std::fs::remove_dir_all(&unpack));
                return Err(format!("I couldn't put {} in place: {e}", p.name));
            }
            std::fs::write(&mark, p.sha256).map_err(|e| format!("I couldn't finish putting {} in place: {e}", p.name))?;
            crate::heard!(std::fs::remove_dir_all(&unpack));
            crate::heard!(std::fs::remove_file(&part));
        }
    }
    if have(p, root) {
        Ok(())
    } else {
        Err(format!("{} is still missing after unpacking", p.name))
    }
}

/// The arguments curl is run with for one piece, before the output file and
/// the address.
///
/// Until 28 Sep 2026 only the connection had a time limit (30 s), so a
/// download that connected and then stalled -- a dropped wifi, a server
/// that stops sending -- waited for ever, and the setup window with it. Now:
/// slower than 10 kB/s for a whole minute counts as stalled and is retried;
/// five retries, on any error, three seconds apart; and a resume (`-C -`)
/// carries on from the bytes already in the `.part` file.
pub fn curl_args() -> Vec<&'static str> {
    vec![
        "-L",
        "--fail",
        "--silent",
        "--show-error",
        "--connect-timeout",
        "30",
        "--speed-limit",
        "10000",
        "--speed-time",
        "60",
        "--retry",
        "5",
        "--retry-all-errors",
        "--retry-delay",
        "3",
        "-C",
        "-",
    ]
}

/// The longest one piece's download may take in all, retries included:
/// ten minutes, plus the time it would take at 100 kB/s. Past that it's
/// stopped and said, never left running.
pub fn deadline_for(bytes: u64) -> Duration {
    Duration::from_secs(600 + bytes / 100_000)
}

fn download(p: &Piece, part: &Path, tools: &Tools, progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    let mut cmd = crate::tools::command(&tools.curl);
    cmd.args(curl_args())
        .arg("-o")
        .arg(part)
        .arg(p.url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "this copy of Windows has no curl.exe, which Windows has included since 2018 — \
                 a Windows update brings it"
                    .to_string()
            } else {
                format!("I couldn't start the download: {e}")
            }
        })?;
    // curl's complaints are read as they come, so a chatty failure can't
    // fill the pipe and hold curl up.
    let mut stderr = child.stderr.take();
    let said = std::thread::spawn(move || {
        let mut err = String::new();
        if let Some(e) = stderr.as_mut() {
            use std::io::Read;
            crate::heard!(e.read_to_string(&mut err));
        }
        err
    });
    let until = std::time::Instant::now() + deadline_for(p.bytes);
    loop {
        let so_far = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
        progress(so_far.min(p.bytes), p.bytes);
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    progress(p.bytes, p.bytes);
                    return Ok(());
                }
                let err = said.join().unwrap_or_default();
                // A resume against a file that is already complete comes
                // back as an error from some servers; the hash check decides.
                if std::fs::metadata(part).map(|m| m.len() == p.bytes).unwrap_or(false) {
                    return Ok(());
                }
                return Err(plain_download_error(p.name, err.trim()));
            }
            Ok(None) if std::time::Instant::now() >= until => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "I couldn't get {}: it was taking far too long ({} minutes), so I stopped it. What arrived is kept \
                     -- try again and it carries on from there",
                    p.name,
                    deadline_for(p.bytes).as_secs() / 60
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(format!("the download of {} stopped: {e}", p.name)),
        }
    }
}

/// curl's complaint, in words a person can act on.
pub fn plain_download_error(name: &str, err: &str) -> String {
    let e = err.to_lowercase();
    if e.contains("could not resolve") || e.contains("couldn't resolve") {
        format!("I couldn't get {name}: this computer doesn't seem to be online")
    } else if e.contains("timed out") || e.contains("timeout") {
        format!("I couldn't get {name}: the connection was too slow and gave up — try again")
    } else if e.contains("404") {
        format!("I couldn't get {name}: it's no longer where it was kept")
    } else if e.contains("no space") || e.contains("failure writing output") || e.contains("disk full") {
        format!("I couldn't get {name}: the disk is full. Free up some space and try again")
    } else if e.contains("ssl") || e.contains("certificate") {
        format!("I couldn't get {name}: the secure connection was refused, often by a firewall or proxy")
    } else if err.is_empty() {
        format!("I couldn't get {name}")
    } else {
        format!("I couldn't get {name}: {err}")
    }
}

fn unzip(zip: &Path, into: &Path, tools: &Tools) -> Result<(), String> {
    // A gzip'd tar (Tor's bundle) or a bzip2'd one (the Kokoro voice,
    // `kokoro`) can't go through `unzip`; `tar` reads both.
    // Windows' own tar.exe (bsdtar) reads zips and gzip, so this only changes
    // the other systems. bzip2 through Windows' tar.exe hasn't been seen on a
    // real Windows machine yet (28 Sep 2026): libarchive supports it, and the
    // Kokoro download is the first to need it.
    let tarball = std::fs::File::open(zip)
        .and_then(|mut f| {
            let mut m = [0u8; 2];
            std::io::Read::read_exact(&mut f, &mut m).map(|_| m)
        })
        .is_ok_and(|m| m == [0x1f, 0x8b] || m == *b"BZ");
    let how = if tarball && tools.unzip == Unzip::Unzip { Unzip::Tar } else { tools.unzip };
    let program = if how == Unzip::Tar && tools.unzip == Unzip::Unzip { "tar".to_string() } else { tools.unzipper.clone() };
    let mut cmd = match how {
        Unzip::Tar => {
            let mut c = crate::tools::command(&program);
            c.arg("-xf").arg(zip).arg("-C").arg(into);
            c
        }
        Unzip::Unzip => {
            let mut c = crate::tools::command(&tools.unzipper);
            c.arg("-o").arg("-q").arg(zip).arg("-d").arg(into);
            c
        }
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|e| format!("I couldn't unpack it: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("I couldn't unpack it: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Put the folder `new` where `live` is, whole: the old one is moved aside,
/// the new one moved in, and the old one put back if that fails -- so `live`
/// is always either all old or all new. A program in the old folder that is
/// running stops the move aside, before anything is touched, and that is
/// said plainly. Anything the old folder had that the new one doesn't is
/// carried over. Where a move can't be made (another drive), it is copied.
pub fn swap_folder(new: &Path, live: &Path) -> std::result::Result<(), String> {
    if let Some(parent) = live.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let name = live.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let aside = live.with_file_name(format!("{name}.old-{}", std::process::id()));
    crate::heard!(std::fs::remove_dir_all(&aside));
    let had = live.exists();
    if had {
        std::fs::rename(live, &aside).map_err(|e| {
            format!("something in {} is in use ({e}) -- close Atlas and anything using it, then try again", live.display())
        })?;
    }
    let moved = std::fs::rename(new, live).or_else(|_| copy_tree(new, live));
    if let Err(e) = moved {
        crate::heard!(std::fs::remove_dir_all(live));
        if had {
            crate::kept!(std::fs::rename(&aside, live));
        }
        return Err(e.to_string());
    }
    if had {
        if let Ok(entries) = std::fs::read_dir(&aside) {
            for e in entries.flatten() {
                let dest = live.join(e.file_name());
                if e.file_name() != std::ffi::OsStr::new(MARKER) && !dest.exists() {
                    crate::kept!(std::fs::rename(e.path(), dest));
                }
            }
        }
        crate::heard!(std::fs::remove_dir_all(&aside));
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

fn slug(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Atlas's own tools folder, put on this process's search path so `ffmpeg`
/// and `ffplay` are found where Atlas put them — no system install, no PATH
/// editing, and nothing outside Atlas's own folder changed.
pub fn use_own_tools(root: &Path) {
    // Never in a test (6 Oct 2026): this changes the whole process's
    // environment. The setup walk's test ran it on a stand-in install whose
    // ffmpeg.exe is the right size and empty, and from then on every test in
    // the run that started ffmpeg got the stand-in -- Windows refused it as
    // corrupt (os error 1392), failing photo editing and the camera. It also
    // pointed CARGO_HOME/RUSTUP_HOME at the stand-in for every later test.
    if crate::roots::under_test() {
        return;
    }
    // And the code checkers Atlas fetched for itself (`codetools`, 1 Oct
    // 2026), with Rust's two folders, which its programs read from the
    // environment.
    for (k, v) in crate::codetools::rust_env(root) {
        std::env::set_var(k, v);
    }
    let mut own: Vec<PathBuf> = ["tools/ffmpeg"].iter().map(|d| root.join(d)).filter(|d| d.is_dir()).collect();
    own.extend(crate::codetools::bin_dirs(root));
    if own.is_empty() {
        return;
    }
    let mut paths: Vec<PathBuf> = own;
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    if let Ok(joined) = std::env::join_paths(paths) {
        std::env::set_var("PATH", joined);
    }
}


/// Whisper's multilingual model (the `.en` ones can't hear other languages
/// at all): what `translate` and `language` need. Hash and size from
/// Hugging Face's own record of the file, 1 Oct 2026.
fn multilingual_listening_model() -> Piece {
    Piece {
        name: "the multilingual listening model",
        for_what: "hearing and translating languages other than English",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        bytes: 147_951_465,
        lands: Lands::File("models/ggml-base.bin"),
    }
}

/// Everything else a capability needs, fetched by Atlas itself after setup's
/// essentials (Eric, 1 Oct 2026: "everything Atlas needs to properly run and
/// perform every capability it has, Atlas needs to download itself"): the
/// meaning model, Parakeet hearing and the wake-word spotter, telling voices
/// apart, seeing, photo cut-outs, the helper (draft) model, the deep brain,
/// and the picture maker. Setup fetches these after the essentials, and the
/// running Atlas tops up whatever is missing (`Daemon::keep_everything_here`).
///
/// Not here: **the "better" talking model** (Qwen3.5 4B). Measured 1 Oct
/// 2026 on the pinned llama.cpp (b10456): every tool call it was offered
/// failed in the server ("failed to parse grammar"), so it answered from
/// nothing -- an invented Friday schedule, "I've added it to your
/// calendar". Fetching it unasked would make Atlas worse.
pub fn everything_else() -> Vec<Piece> {
    let essentials = setup_pieces();
    let mut out: Vec<Piece> = Vec::new();
    for p in [
        understanding(),
        parakeet_pieces(),
        vec![crate::kws::spotter_piece()],
        voice_model(),
        seeing(),
        photos(),
        crate::codetools::tool_pieces(),
        vec![multilingual_listening_model()],
        // Not the draft model: measured four times slower on the laptop
        // (1 Oct 2026, `models.rs` `speculate`), so nothing needs it.
        vec![deep_model()],
        picture_making(),
        // The NPU engine, on a computer with an Intel NPU (item 20).
        crate::npu::pieces(),
        // The coding model this machine has room for, if any (2 Oct 2026).
        crate::coder::pieces_for_here(),
        // curl with HTTP/2, for Apple's push service (item 15).
        crate::apns::curl_piece().into_iter().collect(),
    ]
        .into_iter()
        .flatten()
    {
        if !essentials.iter().chain(out.iter()).any(|a| a.key_path() == p.key_path()) {
            out.push(p);
        }
    }
    out
}

/// Everything setup fetches, in order: the voice pieces, the language model
/// (`pictures`: the same model answers questions and reads screens), then Tor.
/// Here rather than in `setupwin`, which is desktop-only, because the running
/// Atlas answers "what's missing from setup" on every build.
pub fn setup_pieces() -> Vec<Piece> {
    // The Kokoro voice too (29 Sep 2026: Eric, "Atlas sounds like a robot" --
    // piper was the only voice setup fetched, and Kokoro, which Atlas already
    // speaks with when it's there, was an extra nobody knew to ask for).
    catalogue().into_iter().chain(crate::kokoro::pieces()).chain(pictures()).chain(tor()).collect()
}
