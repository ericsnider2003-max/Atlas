/// `println!` that never panics. The background Atlas has no console, and
/// `println!` panics when stdout is a closed pipe ("failed printing to
/// stdout: The pipe is being closed", 30 Sep 2026: Atlas stopped twice that
/// evening). Lost output is fine; a crash isn't.
#[macro_export]
macro_rules! outln {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

/// `eprintln!` that never panics (see `outln!`).
#[macro_export]
macro_rules! errln {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($t)*);
    }};
}

/// `print!` that never panics (see `outln!`).
#[macro_export]
macro_rules! out {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let mut o = std::io::stdout();
        let _ = write!(o, $($t)*);
        let _ = o.flush();
    }};
}

pub mod browser;
pub mod cdp;
pub mod chat;
pub mod clipboard;
pub mod cli;
pub mod courier;
pub mod crash;
pub mod friends;
pub mod wire;
pub mod onion;
pub mod ota;
pub mod handover;
pub mod measure;
pub mod cloudsync;
pub mod clients;
pub mod crew;
pub mod imap;
pub mod msoauth;
pub mod outbox;
pub mod outreach;
pub mod orders;
pub mod preferences;
// Add-ons: Tier 1 of the plugin boundary -- declarative capabilities you
// approve, kept through updates, checked at every step.
pub mod plugins;
// This Atlas's own signing key, introduced to paired devices, so a group's
// owner can sign who is in it and every member can check.
pub mod peerkey;
// Group chats with an owner: who is in them and who may post.
pub mod groups;
// Hearing about updates in the release channel, verified by the release key.
pub mod update_courier;
pub mod update_apply;
pub mod feedback;
// Signing releases so a device can prove an update is really from you before
// it installs it (ed25519). The crypto root the update courier rides on.
pub mod release;
pub mod yourchanges;
pub mod roots;
pub mod smtp;
pub mod texting;
pub mod codes;
pub mod confirmed;
pub mod companion;
pub mod config;
pub mod consolidate;
pub mod consult;
pub mod consent;
pub mod connectivity;
pub mod content;
pub mod draft;
pub mod earned;
pub mod edit;
pub mod editcraft;
pub mod editors;
pub mod endpoint;
pub mod enrol;
pub mod error;
pub mod install;
pub mod intent;
pub mod interrupt;
pub mod layout;
pub mod layout_prefs;
pub mod plainly;
pub mod palette;
pub mod platform;
pub mod capability;
pub mod capture;
pub mod categories;
pub mod certainty;
pub mod checkup;
pub mod channel;
pub mod chain;
pub mod plainchange;
pub mod policy;
pub mod portable;
pub mod publish;
pub mod publishing;
pub mod quickinput;
pub mod reach;
pub mod reclaim;
pub mod meaning;
pub mod meaningroute;
pub mod imagemake;
pub mod selftest;
pub mod mutation;
pub mod regressions;
pub mod coverage;
pub mod cpuuse;
pub mod operate;
pub mod used;
#[cfg(feature = "onnx")]
pub mod meaningnative;
pub mod recall;
pub mod recovery;
pub mod reference;
pub mod references;
pub mod talkbench;
pub mod voicefirst;
pub mod whystopped;
pub mod npu;
pub mod applebrain;
pub mod phonealarms;
pub mod apns;
pub mod applewx;
pub mod onnxfix;
pub mod freeonline;
pub mod parakeet;
pub mod kws;
pub mod report;
pub mod mcpserve;
pub mod codetools;
pub mod getknow;
pub mod keeping;
pub mod weather;
pub mod register;
pub mod rehearse;
pub mod repeating;
pub mod doing;
pub mod presence;
pub mod probe;
pub mod prose;
pub mod profiles;
pub mod remote;
pub mod research;
pub mod resume;
pub mod returning;
pub mod route;
pub mod routine;
pub mod safety;
pub mod sandbox;
pub mod retention;
pub mod thread;
pub mod transport;
pub mod timebox;
pub mod timing;
pub mod tools;
pub mod tray;
pub mod triage;
pub mod telegram;
pub mod tts;
pub mod tune;
pub mod typed;
pub mod uia;
pub mod undo;
pub mod unsub;
pub mod upgrade;
pub mod vault;
pub mod viewing;
pub mod voicepick;
pub mod voice;
pub mod window;
pub mod voiceover;
pub mod voiceid;
pub mod why;
pub mod appearance;
pub mod awareness;
pub mod accounts;
pub mod activity;
pub mod adapt;
pub mod addressing;
pub mod afterme;
pub mod android;
pub mod anticipate;
pub mod answering;
pub mod asking;
pub mod attention;
pub mod audio;
pub mod awake;
pub mod backends;
pub mod backlog;
pub mod b64;
pub mod credentials;
pub mod daemon;
pub mod daily;
pub mod proactive;
pub mod worklog;
pub mod worksession;
pub mod studio;
pub mod applied;
pub mod when;
pub mod notify;
pub mod nudge;
pub mod checks;
pub mod handloop;
pub mod handshape;
pub mod handtrack;
pub mod handweight;
pub mod hollow;
pub mod hollowcode;
pub mod revise;
pub mod onlyone;
pub mod playout;
pub mod winpark;
pub mod unwaited;
pub mod gaze;
pub mod goal;
pub mod tier;
pub mod craft;
pub mod taste;
pub mod motion;
pub mod explain;
pub mod mend;
// How you talk, learned from being corrected, and what Atlas got wrong
// (2 Oct 2026).
pub mod phrasebook;
pub mod misses;
pub mod contents;
pub mod integrations;
pub mod opportunity;
// Finding opportunities on a polite daily schedule, and the daemon's side of it.
pub mod hunt;
pub mod hunting;
pub mod dash;
pub mod decide;
pub mod trace;
pub mod twofactor;
pub mod webrun;
pub mod astype;
pub mod later;
pub mod facts;
pub mod faithful;
pub mod council;
pub mod build_it;
pub mod coding_agent;
// The model code is written with, swapped in for a build (2 Oct 2026).
pub mod coder;
// Reading a project for a code change: the files and pieces that matter.
pub mod projectread;
pub mod brief;
pub mod filing;
// Sorting a folder by kind, copies and old installers to "To review", on
// one yes and undone by "undo that" (2 Oct 2026).
pub mod organize;
pub mod files;
pub mod pdftext;
pub mod hotkeys;
pub mod typebox;
pub mod unpack;
pub mod finance;
pub mod frames;
pub mod freshness;
pub mod fit;
pub mod firstrun;
pub mod flow;
pub mod gguf;
pub mod handoff;
pub mod health;
pub mod hlc;
pub mod hearing;
// A quiet voice heard without shouting: speech judged against the room and
// levelled before speech-to-text; Windows' input level read and raised once
// when it is set too low (30 Sep 2026).
pub mod leveller;
pub mod miclevel;
pub mod goingaway;
pub mod goodbye;
pub mod grade;
// Photo editing on a copy (29 Sep 2026): ffmpeg does the work, `straighten`
// measures the tilt, `cutout` finds the subject (tract, behind `onnx`).
pub mod photo;
pub mod straighten;
pub mod cutout;
pub mod grading;
pub mod grants;
pub mod household;
pub mod http;
pub mod hub;
pub mod hubpages;
pub mod sound;
pub mod mobile;
pub mod oslook;
pub mod hublive;
pub mod hubjobs;
pub mod hubvault;
pub mod identity;
pub mod improve;
pub mod ios;
pub mod infer;
pub mod vision;
pub mod words;
pub mod mark;
pub mod market;
pub mod asia;
pub mod rollover;
pub mod stale;
pub mod refusals;
pub mod fxday;
pub mod standdown;
pub mod together;
pub mod levels;
pub mod untrusted;
pub mod digest;
pub mod live;
pub mod firewall;
pub mod doorrule;
pub mod phonemodel;
pub mod index;
pub mod input;
pub mod lanes;
pub mod knowhow;
pub mod judgment;
pub mod kin;
/// Kokoro, the better voice, spoken inside Atlas through sherpa-onnx's C library.
pub mod kokoro;
pub mod roster;
pub mod shared_task;
pub mod language;
pub mod learned;
pub mod ledger;
pub mod lifecycle;
pub mod log;
pub mod look;
// The desktop panel's palette and mark geometry — egui-typed (Color32), so it
// rides with the desktop UI and is dropped from a mobile/headless core build.
#[cfg(feature = "desktop-ui")]
pub mod look_paint;
pub mod online;
pub mod ocr;
pub mod overnight;
pub mod opsec;
pub mod otherside;
pub mod overlay;
pub mod panel;
pub mod perf;
pub mod person;
pub mod pipeline;
pub mod persona;
// How much of a smart-ass Atlas may be, and the fence around it.
pub mod wit;
pub mod talkback;
pub mod firstlaunch;
pub mod getpieces;
pub mod glance;
pub mod phone;
pub mod phoneadd;
pub mod phonelink;
pub mod mail;
pub mod memory;
pub mod messaging;
pub mod money;
pub mod mesh;
pub mod nearby;
pub mod metrics;
pub mod mind;
pub mod models;
/// Two brains: the talking model and a deeper one for background work (30 Sep 2026).
pub mod deepbrain;
pub mod modes;
pub mod scheduler;
pub mod selfaudit;
pub mod selfgrant;
pub mod selfwork;
pub mod server;
pub mod hubwin;
pub mod speaking;
// Their drawing is eframe, so it is only in the desktop build, like
// `setupwin` and the panel; the rest of each (keeping settings, the
// overlay's stages) is in the GUI-free core too.
pub mod overlaywin;
pub mod next_up;
pub mod phases;
pub mod picture_talk;
// "Can you see me?" read as a request to look through the camera (30 Sep 2026).
pub mod callmute;
pub mod camera_ask;
pub mod camwatch;
pub mod growth;
pub mod callwatch;
pub mod callrec;
pub mod callnotes;
pub mod localclock;
#[cfg(all(windows, target_env = "gnu"))]
pub mod webview2_loader;
pub mod settingswin;
// The first-launch setup window — eframe/egui, desktop only.
#[cfg(feature = "desktop-ui")]
pub mod setupwin;
pub mod startup;
// Atlas's icon by the clock, owned by the background Atlas (Windows).
pub mod notifyicon;
pub mod speaker;
pub mod speakernet;
pub mod settings;
pub mod session;
pub mod shakedown;
pub mod signals;
pub mod speech;
pub mod spoken_form;
pub mod signin;
pub mod stance;
pub mod store;
pub mod strategy;
pub mod subject;
pub mod sync;
pub mod system;
pub mod booking;
pub mod calendar;
pub mod brain;
pub mod budget;
pub mod delegate;
pub mod delivery;
pub mod diagnose;
pub mod dictate;
pub mod doctor;
pub mod wants;
pub mod watch;
pub mod wanted;
pub mod whichone;
pub mod wireguard;
pub mod which_errand;
pub mod walkthrough;
pub mod watching;
pub mod workingset;
pub mod workshop;
pub mod workspace;
pub mod workspace_view;
pub mod ws;
pub mod elsewhere;
pub mod understood;
pub mod automation;
pub mod bm25;
pub mod router;
pub mod backed;
pub mod taskloop;
pub mod streams;
pub mod chunker;
pub mod civil;
pub mod cronspec;
pub mod linkage;
pub mod mailthread;
pub mod ratelimit;
pub mod readable;
pub mod recur;
pub mod sealedlog;
pub mod stemmer;
pub mod typos;
pub mod urgency;
pub mod vformat;

// Round 3 ports (23 Sep 2026, late).
pub mod agefile;
pub mod bandit;
pub mod diarize;
pub mod diff;
pub mod drain;
pub mod guessable;
pub mod lookalike;
pub mod pronounce;
pub mod redact;
pub mod yata;
pub mod zipread;
pub mod tz;
pub mod vad;
pub mod mfcc;
pub mod gmm;
pub mod wakeword;
pub mod micthread;
pub mod utterance;
pub mod speakthread;
pub mod vadcal;
pub mod hotkey;
pub mod inhibit;
pub mod loginseal;
pub mod toast;
pub mod spoken_numbers;
pub mod fixloop;
pub mod cutcheck;
pub mod pngcodec;
pub mod gifenc;
pub mod filmstrip;
pub mod meshio;
pub mod scene3d;
pub mod marketdays;
pub mod cliphist;
pub mod screentext;
pub mod mailbook;
// Other programs' tools, over the Model Context Protocol (28 Sep 2026).
pub mod mcp;
// Reading mail through the Himalaya program, when chosen (28 Sep 2026).
pub mod himalaya;
pub mod waitingfor;
pub mod launcher;
pub mod tradeday;
pub mod meetprep;
pub mod snippets;
pub mod findfile;
pub mod pdfkit;
pub mod people;
pub mod feeds;
// Your social accounts' numbers kept over time, and what's working for the
// people you watch, from free official routes only (29 Sep 2026).
pub mod social;
pub mod receipts;
pub mod habits;
pub mod srs;
pub mod translation;
pub mod chords;
pub mod workday;
