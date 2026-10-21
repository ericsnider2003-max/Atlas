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

pub mod artifact;
pub mod browser;
pub mod cdp;
pub mod chat;
pub mod cli;
pub mod clients;
pub mod clipboard;
pub mod cloudsync;
pub mod courier;
pub mod crash;
pub mod crew;
pub mod doorbell;
pub mod friends;
pub mod handover;
pub mod imap;
pub mod job;
pub mod measure;
pub mod msoauth;
pub mod onion;
pub mod orders;
pub mod ota;
pub mod outbox;
pub mod outreach;
pub mod preferences;
pub mod wire;
// Add-ons: Tier 1 of the plugin boundary -- declarative capabilities you
// approve, kept through updates, checked at every step.
pub mod plugins;
// This Atlas's own signing key, introduced to paired devices, so a group's
// owner can sign who is in it and every member can check.
pub mod peerkey;
// Group chats with an owner: who is in them and who may post.
pub mod groups;
// Hearing about updates in the release channel, verified by the release key.
pub mod feedback;
pub mod update_apply;
pub mod update_courier;
// Signing releases so a device can prove an update is really from you before
// it installs it (ed25519). The crypto root the update courier rides on.
pub mod accounts;
pub mod activity;
pub mod adapt;
pub mod addressing;
pub mod afterme;
pub mod android;
pub mod answering;
pub mod anticipate;
pub mod apns;
pub mod appearance;
pub mod applebrain;
pub mod applewx;
pub mod applied;
pub mod asking;
pub mod attention;
pub mod audio;
pub mod awake;
pub mod awareness;
pub mod b64;
pub mod backends;
pub mod backlog;
pub mod capability;
pub mod capture;
pub mod categories;
pub mod certainty;
pub mod chain;
pub mod channel;
pub mod checks;
pub mod checkup;
pub mod codes;
pub mod codetools;
pub mod companion;
pub mod config;
pub mod confirmed;
pub mod connect;
pub mod connecting;
pub mod connection_removal;
pub mod connectivity;
pub mod connectors;
pub mod consent;
pub mod consolidate;
pub mod consult;
pub mod contemplate;
pub mod content;
pub mod creative;
pub mod coverage;
pub mod cpuuse;
pub mod craft;
pub mod credentials;
pub mod daemon;
pub mod daily;
pub mod doing;
pub mod draft;
pub mod earned;
pub mod edit;
pub mod editcraft;
pub mod editors;
pub mod endpoint;
pub mod enrol;
pub mod error;
pub mod evidence;
pub mod explain;
pub mod freeonline;
pub mod gaze;
pub mod getknow;
pub mod goal;
pub mod handloop;
pub mod handshape;
pub mod handtrack;
pub mod handweight;
pub mod hollow;
pub mod hollowcode;
pub mod imagemake;
pub mod install;
pub mod intent;
pub mod interrupt;
pub mod keeping;
pub mod kws;
pub mod layout;
pub mod layout_prefs;
pub mod mcpserve;
pub mod meaning;
#[cfg(feature = "onnx")]
pub mod meaningnative;
pub mod meaningroute;
pub mod mend;
pub mod motion;
pub mod muse;
pub mod mutation;
pub mod notify;
pub mod npu;
pub mod nudge;
pub mod oauthlink;
pub mod onlyone;
pub mod onnxfix;
pub mod operate;
pub mod ownspeech;
pub mod palette;
pub mod parakeet;
pub mod phonealarms;
pub mod phonemode;
pub mod plainchange;
pub mod plainly;
pub mod platform;
pub mod playout;
pub mod policy;
pub mod portable;
pub mod presence;
pub mod proactive;
pub mod probe;
pub mod profiles;
pub mod prose;
pub mod publish;
pub mod publishing;
pub mod quickinput;
pub mod reach;
pub mod recall;
pub mod reclaim;
pub mod recovery;
pub mod reference;
pub mod references;
pub mod register;
pub mod regressions;
pub mod rehearse;
pub mod release;
pub mod remote;
pub mod repeating;
pub mod report;
pub mod research;
pub mod resume;
pub mod retention;
pub mod returning;
pub mod revise;
pub mod roots;
pub mod route;
pub mod routine;
pub mod safety;
pub mod sandbox;
pub mod selftest;
pub mod smtp;
pub mod studio;
pub mod talkbench;
pub mod taste;
pub mod telegram;
pub mod texting;
pub mod thread;
pub mod tier;
pub mod timebox;
pub mod timing;
pub mod tools;
pub mod transport;
pub mod tray;
pub mod triage;
pub mod tts;
pub mod tune;
pub mod typed;
pub mod uia;
pub mod undo;
pub mod unsub;
pub mod unwaited;
pub mod upgrade;
pub mod used;
pub mod vault;
pub mod viewing;
pub mod voice;
pub mod voicefirst;
pub mod voiceid;
pub mod voicelat;
pub mod voiceover;
pub mod voicepick;
pub mod watchvideo;
pub mod weather;
pub mod webpush;
pub mod when;
pub mod why;
pub mod whystopped;
pub mod window;
pub mod winpark;
pub mod worklog;
pub mod worksession;
pub mod yourchanges;
// How you talk, learned from being corrected, and what Atlas got wrong
// (2 Oct 2026).
pub mod contents;
pub mod integrations;
pub mod misses;
pub mod opportunity;
pub mod phrasebook;
// Finding opportunities on a polite daily schedule, and the daemon's side of it.
pub mod astype;
pub mod bisect;
pub mod build_it;
pub mod coding_agent;
pub mod council;
pub mod dash;
pub mod decide;
pub mod facts;
pub mod faithful;
pub mod hunt;
pub mod hunting;
pub mod later;
pub mod scaffold;
pub mod trace;
pub mod twofactor;
pub mod webrun;
// The model code is written with, swapped in for a build (2 Oct 2026).
pub mod coder;
// Reading a project for a code change: the files and pieces that matter.
pub mod brief;
pub mod filing;
pub mod projectread;
// Sorting a folder by kind, copies and old installers to "To review", on
// one yes and undone by "undo that" (2 Oct 2026).
pub mod files;
pub mod finance;
pub mod firstrun;
pub mod fit;
pub mod flow;
pub mod frames;
pub mod freshness;
pub mod gguf;
pub mod handoff;
pub mod health;
pub mod hearing;
pub mod hlc;
pub mod hotkeys;
pub mod organize;
pub mod pdftext;
pub mod typebox;
pub mod unpack;
// A quiet voice heard without shouting: speech judged against the room and
// levelled before speech-to-text; Windows' input level read and raised once
// when it is set too low (30 Sep 2026).
pub mod goingaway;
pub mod goodbye;
pub mod grade;
pub mod leveller;
pub mod miclevel;
// Photo editing on a copy (29 Sep 2026): ffmpeg does the work, `straighten`
// measures the tilt, `cutout` finds the subject (tract, behind `onnx`).
pub mod asia;
pub mod cutout;
pub mod digest;
pub mod doorrule;
pub mod firewall;
pub mod fxday;
pub mod grading;
pub mod grants;
pub mod household;
pub mod http;
pub mod hub;
pub mod hubjobs;
pub mod hublive;
pub mod hubpages;
pub mod hubvault;
pub mod identity;
pub mod improve;
pub mod index;
pub mod infer;
pub mod input;
pub mod ios;
pub mod judgment;
pub mod kin;
pub mod knowhow;
/// Kokoro, the better voice, spoken inside Atlas through sherpa-onnx's C library.
pub mod kokoro;
pub mod lanes;
pub mod language;
pub mod learned;
pub mod ledger;
pub mod levels;
pub mod lifecycle;
pub mod live;
pub mod log;
pub mod look;
pub mod mark;
pub mod market;
pub mod mobile;
pub mod oslook;
pub mod phonemodel;
pub mod photo;
pub mod refusals;
pub mod rollover;
pub mod roster;
pub mod shared_task;
pub mod sound;
pub mod stale;
pub mod standdown;
pub mod straighten;
pub mod together;
pub mod untrusted;
pub mod vision;
pub mod words;
// The desktop panel's palette and mark geometry — egui-typed (Color32), so it
// rides with the desktop UI and is dropped from a mobile/headless core build.
#[cfg(feature = "desktop-ui")]
pub mod look_paint;
pub mod ocr;
pub mod online;
pub mod opsec;
pub mod otherside;
pub mod overlay;
pub mod overnight;
pub mod panel;
pub mod perf;
pub mod person;
pub mod persona;
pub mod pipeline;
// How much of a smart-ass Atlas may be, and the fence around it.
/// Two brains: the talking model and a deeper one for background work (30 Sep 2026).
pub mod deepbrain;
pub mod firstlaunch;
pub mod getpieces;
pub mod glance;
pub mod gpulost;
pub mod hubwin;
pub mod mail;
pub mod memory;
pub mod mesh;
pub mod messaging;
pub mod metrics;
pub mod mind;
pub mod models;
pub mod modes;
pub mod money;
pub mod nearby;
pub mod phone;
pub mod phoneadd;
pub mod phonelink;
pub mod scheduler;
pub mod selfaudit;
pub mod selfgrant;
pub mod selfwork;
pub mod server;
pub mod speaking;
pub mod talkback;
pub mod wit;
// Their drawing is eframe, so it is only in the desktop build, like
// `setupwin` and the panel; the rest of each (keeping settings, the
// overlay's stages) is in the GUI-free core too.
pub mod next_up;
pub mod overlaywin;
pub mod phases;
pub mod picture_talk;
// "Can you see me?" read as a request to look through the camera (30 Sep 2026).
pub mod callmute;
pub mod callnotes;
pub mod callrec;
pub mod callwatch;
pub mod camera_ask;
pub mod camwatch;
pub mod childjob;
pub mod growth;
pub mod localclock;
pub mod settingswin;
#[cfg(all(windows, target_env = "gnu"))]
pub mod webview2_loader;
// The first-launch setup window — eframe/egui, desktop only.
#[cfg(feature = "desktop-ui")]
pub mod setupwin;
pub mod startup;
// Atlas's icon by the clock, owned by the background Atlas (Windows).
pub mod automation;
pub mod backed;
pub mod bm25;
pub mod booking;
pub mod brain;
pub mod budget;
pub mod calendar;
pub mod chunker;
pub mod civil;
pub mod cronspec;
pub mod delegate;
pub mod delivery;
pub mod diagnose;
pub mod dictate;
pub mod doctor;
pub mod elsewhere;
pub mod linkage;
pub mod mailthread;
pub mod notifyicon;
pub mod ratelimit;
pub mod readable;
pub mod recur;
pub mod router;
pub mod sealedlog;
pub mod session;
pub mod settings;
pub mod shakedown;
pub mod signals;
pub mod signin;
pub mod speaker;
pub mod speakernet;
pub mod speech;
pub mod splitter;
pub mod spoken_form;
pub mod stance;
pub mod stemmer;
pub mod store;
pub mod strategy;
pub mod streams;
pub mod subject;
pub mod sync;
pub mod system;
pub mod taskloop;
pub mod typos;
pub mod understood;
pub mod unheard;
pub mod urgency;
pub mod vformat;
pub mod walkthrough;
pub mod wanted;
pub mod wants;
pub mod watch;
pub mod watching;
pub mod which_errand;
pub mod whichone;
pub mod wireguard;
pub mod workingset;
pub mod workshop;
pub mod workspace;
pub mod workspace_view;
pub mod ws;

// Round 3 ports (23 Sep 2026, late).
pub mod agefile;
pub mod bandit;
pub mod cliphist;
pub mod cutcheck;
pub mod diarize;
pub mod diff;
pub mod drain;
pub mod filmstrip;
pub mod fixloop;
pub mod gifenc;
pub mod gmm;
pub mod guessable;
pub mod hotkey;
pub mod inhibit;
pub mod loginseal;
pub mod lookalike;
pub mod mailbook;
pub mod marketdays;
pub mod meshio;
pub mod mfcc;
pub mod micthread;
pub mod pngcodec;
pub mod pronounce;
pub mod redact;
pub mod scene3d;
pub mod screentext;
pub mod speakthread;
pub mod spoken_numbers;
pub mod toast;
pub mod tz;
pub mod utterance;
pub mod vad;
pub mod vadcal;
pub mod wakeword;
pub mod yata;
pub mod zipread;
// Other programs' tools, over the Model Context Protocol (28 Sep 2026).
pub mod mcp;
// Reading mail through the Himalaya program, when chosen (28 Sep 2026).
pub mod feeds;
pub mod findfile;
pub mod himalaya;
pub mod launcher;
pub mod meetprep;
pub mod pdfkit;
pub mod people;
pub mod snippets;
pub mod tradeday;
pub mod waitingfor;
// Your social accounts' numbers kept over time, and what's working for the
// people you watch, from free official routes only (29 Sep 2026).
pub mod chords;
pub mod habits;
pub mod receipts;
pub mod social;
pub mod srs;
pub mod translation;
pub mod workday;
