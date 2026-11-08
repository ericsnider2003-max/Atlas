# The correction log: format, as implemented (2026-10-23)

For the training session. Everything here is what `src/corrlog.rs` and
`src/daemon/correction_log.rs` do; nothing is planned. Schema version **1**.

## Where it is

`<install>/data/learning/corrections.jsonl` (`Store::data_dir()` + `learning/corrections.jsonl`).
It is **laptop only**: it is beside `data/state/`, not inside it, and `learning` is not one of the
folders a backup keeps (`safety::OWNED_FOLDERS` = notes, calls, reading, trash, recovery), so it is
in no backup, no restore and nothing a peer is sent. No code reads it except the daemon (to append
and to forget) and the nightly job. `tests/the_correction_log.rs::the_log_is_not_in_a_backup_or_a_sync`
checks that the backup and sync sources never name it. A second person's profile
(`data/state/profiles/<id>`) writes nothing.

Encoding UTF-8, one JSON object per line, `\n` terminated. Read it tolerantly: a damaged line is
skipped, and unknown fields should be ignored (a later schema adds fields, never changes these).

## A line

```json
{"v":1,"id":"03af08c43513892e","ts":1790900010,"kind":"correction",
 "said":"no, I meant open spotify",
 "prev":{"said":"open notepad","reply":"Notepad is up.","intent":"open_app"},
 "context":{"app":"explorer.exe","recent":["clock","open_app"],"source":"phrase"},
 "did":{"intent":"open_app","detail":"notepad"},
 "right":{"intent":"open_app","detail":"spotify","from":"owner_said"},
 "ask":null,
 "redacted":0}
```

| field | type | meaning |
|---|---|---|
| `v` | int | schema version, 1 |
| `id` | string | 16 hex characters, random; what "forget that" removes by |
| `ts` | int | unix seconds of the turn that wrote it |
| `kind` | string | `correction`, `undo`, `approval`, `rejection`, `clarified` |
| `said` | string | the owner's words that carried the signal (the "no, I meant ..." / "undo" / "yes" / the answer). Redacted, at most 400 characters |
| `prev.said` | string | the owner's turn before (the one being corrected, asked about or undone). Redacted, <= 400 chars. Empty when there was none within 3 minutes |
| `prev.reply` | string | Atlas's reply to that turn (for approval/rejection, the question it asked). Redacted, <= 300 chars. Empty when the turn rested on outside text |
| `prev.intent` | string or null | the command name Atlas ran for that turn (`session::kind_of`, the `commands.yaml` name), null if none |
| `context.app` | string | the foreground **process file name** when the turn began (e.g. `notepad.exe`), never a window title; empty if unknown. For a correction/undo it is the corrected turn's |
| `context.recent` | string[] | up to the last 3 command names Atlas ran before that turn (not counting unknown/say/ask/undo) |
| `context.source` | string | how the turn was read: `phrase` (a phrase or learned wording matched), `model` (the language model chose), `floor` (no model loaded; plain code), `ask` (the turn answered a question Atlas asked), `none` (it did nothing). `fast` is reserved: there is no separate fast layer today, so it is never written. Best effort, derived after the turn |
| `did.intent` | string or null | the command Atlas acted on or proposed |
| `did.detail` | string or null | short redacted target, <= 80 chars (the command's own argument, else its plain-words description) |
| `right.intent` | string or null | the correct command, or null if unknown/none |
| `right.detail` | string or null | its target, redacted, <= 80 chars |
| `right.from` | string | `owner_said`, `owner_answer`, `undo`, `approval`, `rejection` |
| `ask` | object or null | only for `clarified`: `{"question": "...", "options": ["..."]}` (question <= 300 chars, up to 6 options each <= 80) |
| `redacted` | int | how many spans were replaced by placeholders in this line's text fields |

### The kinds

- `correction`: "no, I meant X" / "I said X" (written when X has run: `did` = what Atlas did,
  `right` = what X did, `from` = `owner_said`); or "that's not what I asked" (held; written when the
  next turn says what was right, `right` filled and `from` = `owner_answer`, or at the turn after
  that with `right.intent` null and `from` = `owner_said`).
- `undo`: the owner undid what Atlas had just done (within 120 s). `did` = what was undone,
  `right.intent` null, `from` = `undo`.
- `approval` / `rejection`: a yes or no to a question Atlas asked before acting: the parked
  approvals (`session` `Pending::Approval`, including several at once: "yes to both", "no to the
  second"; and the permission-grant questions) and "Undo X?". `did` = the asked command; `right` =
  the same command for an approval (`from` = `approval`), null for a rejection (`from` = `rejection`).
  Other yes/no offers (a proactive offer, a sorting plan, a flow's wait) are not logged.
- `clarified`: Atlas asked a specific question because it was unsure (a `Choose` or `Missing`
  resolution, an AskClarification, the which-app/which-file question built from context), and the
  owner's next turn produced an action. `ask` = the question and what it offered; `did` empty;
  `right` = the command and target the answer produced, `from` = `owner_answer`, `source` = `ask`.
  This is the labelled example. (The errand "which one" question is not logged yet.)

## What is never in it

- Every text field is redacted (`redact::Scrubber`: private keys, AWS/GitHub/Slack/API keys, JWTs,
  high-entropy strings, card numbers, IBANs, SSNs, email addresses, phone numbers become `⟦TAG_n⟧`).
- After a word like "password", "passcode", "passphrase", "pin", "otp", "secret", "token", "pwd",
  "2fa" (and "code" or "key" when a digit, a spoken digit or "is ..." follows), everything to the end
  of that clause (a full stop, semicolon, `!`, `?` or "and then") is a secret ("the password for
  gmail is hunter2", "pin number is 1234", "code is 4 8 2 9"). A line with a secret in any text it
  would keep (`said`, `prev.said`, `prev.reply`, the details, the question and options) is **not
  written at all**; it is not edited. Only words after the keyword are caught ("Hunter2 is the
  password" is not).
- A turn that handled a secret writes **nothing**: commands `sign_in`, `create_account`,
  `type_code`, `two_factor`, `set_key`, `unlock`, `pair`, `accept_pairing`, `hand_over`,
  `take_it_back`, or any name containing vault/signing/passphrase/credential/link_account/sign_up,
  whether as the turn's command, the previous turn's, the proposed one or among `recent`. - Nothing is written while a masked read (`typed::ask_quietly`) is open (`typed::masked_read_active`).
  Those reads also never reach a turn.
- A turn that rested on text someone else wrote (SEC2: `tainted_by_outside`, or an outside read
  during the turn) keeps only the owner's words: `prev.reply` is empty, `did.detail`,
  `right.detail` and `ask` are null.
- Nothing while Atlas is handed over to someone else.
- Commands a model invented and the gate turned away (`intent::gate_model_call`) are only noted in
  the ordinary log (`debug: not run, not a command Atlas has (handed on): ...`), once per turn, not here.

## Bounds

At most 5 MB (5,242,880 bytes) and 20,000 lines (a line is about 350 bytes or more, so the byte
bound is reached first). Over either, the oldest lines are dropped and the file rewritten
atomically (`store::write_whole`) down to 90% of both bounds. One line is at most 4,096 bytes (not
counting the newline): fields are cut, longest first, until it fits; a line that still would not is
not written. Appending is one small write; a failed write is logged once in `atlas.log` and never
shown as a turn failure. Append, trim and every rewrite hold a lock file
(`corrections.jsonl.lock`, exclusive create, waits up to 5 s, one older than 30 s is a crashed
holder's) so two Atlas processes cannot lose each other's lines.

## Forgetting, and turning it off

By voice or typing (handled before anything else reads the words, in `how_you_talk_turn`):

- "forget that", "forget the last thing", "forget what I just said": removes the lines written in
  this chain (lines written within 3 minutes of each other since the daemon started), or the newest
  line if the chain is gone (after a restart).
- "forget what I said about X" / "forget everything I said about X": removes every line with X (any
  case) in any text field.
- "forget everything", "clear what you've learned": empties the file.
- Every forget also clears what Atlas holds in memory about the words (the last turn a correction
  would be about, a held "that's wrong", a pending question), so the next "no, I meant ..." cannot
  write them back.
- The phrasebook's existing commands do the same on top of their own: "forget that phrase" and
  "unlearn that" also drop this chain's lines, "forget the phrase X" drops lines mentioning X, and
  "forget everything you learned about how I talk" / "clear my phrasebook" empties the file.
- "stop learning from me": nothing is written from then on, and the setting is kept across
  restarts (store record `learning_log`, `{"on": false}`). "start learning again" turns it back on.
  Lines already there stay until forgotten.

## Implementation map

- `src/corrlog.rs`: schema, redaction, builder (`build`), file (`Log`), switch and chain (`Keeper`),
  the spoken asks (`asked`).
- `src/daemon/correction_log.rs`: what a turn writes; `src/daemon/learning.rs` calls it from
  `turn_watched` and `meant_this_instead`.
- Tests: `corrlog::tests` (lib) and `tests/the_correction_log.rs`.
