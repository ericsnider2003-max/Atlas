#!/usr/bin/env node
// filing.mjs — keeps a project's "what is current / what is next / where things live" true.
// One file, Node 18+, zero dependencies. Runs the same from PowerShell, cmd, bash and zsh:
//   node .filing/filing.mjs <command> [...]      (help: node .filing/filing.mjs help)
// Scripts move, hash, lock and check. People decide what is current, what is deleted, what is merged.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const VERSION = '4.2.0';
const P = path.posix;
const SELF = fileURLToPath(import.meta.url);
const FRONT_DOORS = new Set(['agents.md', 'claude.md', 'gemini.md', 'agents.override.md']);
const ROOT_KEEP = new Set(['readme.md', 'changelog.md', 'license', 'license.md', 'contributing.md', 'security.md',
  'code_of_conduct.md', 'agents.md', 'claude.md', 'gemini.md']);
const DOC_EXT = new Set(['.md', '.markdown', '.txt', '.pdf', '.docx', '.doc', '.odt', '.rtf']);
const TEXT_EXT = new Set(['.md', '.markdown', '.txt', '.json', '.yaml', '.yml', '.toml', '.ini', '.cfg', '.conf', '.xml',
  '.ps1', '.psm1', '.psd1', '.bat', '.cmd', '.sh', '.bash', '.zsh', '.py', '.js', '.mjs', '.cjs', '.ts', '.tsx', '.jsx',
  '.go', '.rs', '.rb', '.java', '.cs', '.php', '.sql', '.service', '.timer', '.csv', '.tsv', '.html', '.css', '.vue']);
const CODE_EXT = new Set([...TEXT_EXT].filter((e) => !['.md', '.markdown', '.txt', '.csv', '.tsv', '.html', '.css'].includes(e)));
const CODE_NAMES = /^(dockerfile|makefile|procfile|crontab|justfile|.*\.(service|timer|plist|nssm))$/i;
const SECRET_NAME = /^(\.env(\.(?!example$|sample$|template$|dist$)[\w.-]+)?|id_(rsa|dsa|ecdsa|ed25519)|credentials[\w.-]*\.json|.*\.(pem|key|pfx|p12|jks|keystore|ppk)|(.*[_.-])?secrets?([_.-].*)?)$/i;
const TRANSFER = /\.(zip|7z|rar|tar|tgz|gz|bz2|xz|bundle)$/i;
const BUILD_DIRS = new Set(['dist', 'build', 'out', 'bin', 'obj', '__pycache__', '.next', 'target', 'coverage', '.pytest_cache', '.venv', 'venv', 'node_modules']);
const RESERVED = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\..*)?$/i;
const DATE_RE = /\b(\d{4}-\d{2}-\d{2})\b/;
const BLOCK_BEGIN = '<!-- filing:begin';
const BLOCK_END = '<!-- filing:end -->';
const LEASE_TTL = 30000;
const POLL_MS = 50;

export const DEFAULTS = {
  version: 1,
  layout: { docs: 'docs', current: 'docs/STATUS.md', index: 'docs/INDEX.md', archive: 'docs/archive', sessions: 'docs/sessions' },
  docGlobs: ['*.md', 'docs/**/*.md'],
  docExclude: ['docs/archive/**', 'docs/sessions/**'],
  packages: [],
  pinned: [],
  main: '',
  owner: '',
};

// ---------------------------------------------------------------- small helpers
const sleep = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
const sha = (buf) => crypto.createHash('sha256').update(buf).digest('hex');
const today = (d = new Date()) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
const rand = (n = 3) => crypto.randomBytes(n).toString('hex');
const who = () => process.env.FILING_WHO || 'agent';
class Fail extends Error { constructor(msg, code = 1) { super(msg); this.exit = code; } }

/** A path relative to root, always with forward slashes. `mod` lets tests use path.win32. */
export function relPosix(root, abs, mod = path) { return mod.relative(root, abs).split(mod.sep).join('/'); }
/** Absolute OS path for a POSIX-relative path. */
export function absOf(root, rel, mod = path) { return mod.join(root, ...rel.split('/')); }
const exists = (p) => { try { fs.statSync(p); return true; } catch { return false; } };
const isFile = (p) => { try { return fs.statSync(p).isFile(); } catch { return false; } };
const readText = (p) => { try { return fs.readFileSync(p, 'utf8'); } catch { return null; } };
const lf = (s) => (s == null ? s : s.replace(/\r\n/g, '\n'));

/** Retry a filesystem call on Windows-style transient locks (antivirus, indexers). */
function retry(fn) {
  for (let i = 0, wait = 50; ; i++, wait *= 2) {
    try { return fn(); } catch (e) {
      if (i >= 5 || !['EBUSY', 'EPERM', 'EACCES'].includes(e.code)) throw e;
      sleep(wait);
    }
  }
}
/** Write via a temp file in the SAME folder, then rename (atomic on one volume). */
export function writeAtomic(file, data) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.tmp-${process.pid}-${rand()}`;
  fs.writeFileSync(tmp, data);
  retry(() => fs.renameSync(tmp, file));
}
function writeIfChanged(file, text) {
  if (lf(readText(file)) === text) return false;
  writeAtomic(file, text);
  return true;
}

/** Glob → RegExp. Supports **, *, ? and a trailing / for directories. */
export function globRe(g) {
  let s = '';
  for (let i = 0; i < g.length; i++) {
    const c = g[i];
    if (c === '*' && g[i + 1] === '*') { s += g[i + 2] === '/' ? '(?:.*/)?' : '.*'; i += g[i + 2] === '/' ? 2 : 1; }
    else if (c === '*') s += '[^/]*';
    else if (c === '?') s += '[^/]';
    else if (c === '[' && g.indexOf(']', i) > i) { const j = g.indexOf(']', i); s += `[${g.slice(i + 1, j).replace(/^!/, '^').replace(/\\/g, '\\\\')}]`; i = j; }
    else s += c.replace(/[.+^${}()|[\]\\]/g, '\\$&');
  }
  return new RegExp(`^${s}$`);
}
const anyGlob = (globs, rel) => globs.some((g) => globRe(g).test(rel));

/** Minimal .gitignore matcher for non-git folders (root .gitignore only; negation supported). */
export function gitignoreMatcher(text) {
  const rules = [];
  for (let line of (text || '').split(/\r?\n/)) {
    if (!line.trim() || line.startsWith('#')) continue;
    line = line.replace(/\s+$/, '');
    const neg = line.startsWith('!'); if (neg) line = line.slice(1);
    const dirOnly = line.endsWith('/'); if (dirOnly) line = line.slice(0, -1);
    const anchored = line.startsWith('/') || line.includes('/'); line = line.replace(/^\//, '');
    const re = globRe(anchored ? line : `**/${line}`);
    rules.push({ neg, dirOnly, re });
  }
  return (rel, isDir) => {
    let ignored = false;
    const parts = rel.split('/');
    for (let i = 1; i <= parts.length; i++) {
      const sub = parts.slice(0, i).join('/');
      const subIsDir = i < parts.length || isDir;
      for (const r of rules) if ((!r.dirOnly || subIsDir) && r.re.test(sub)) ignored = !r.neg;
      if (ignored && i < parts.length) return true; // an ignored parent dir cannot be re-included
    }
    return ignored;
  };
}

// ---------------------------------------------------------------- git
function git(root, args, opts = {}) {
  const r = spawnSync('git', ['-c', 'core.quotepath=off', ...args], { cwd: root, encoding: 'utf8', maxBuffer: 256 << 20, input: opts.input, timeout: opts.timeout });
  if (r.error && r.error.code === 'ENOENT') return { ok: false, out: '', err: 'git not installed' };
  return { ok: r.status === 0, out: r.stdout || '', err: (r.stderr || '') + (r.error ? String(r.error.message) : '') };
}
export function isGit(root) { const r = git(root, ['rev-parse', '--is-inside-work-tree']); return r.ok && r.out.trim() === 'true'; }

// ---------------------------------------------------------------- project context
export function findRoot(start) {
  if (path.basename(path.dirname(SELF)) === '.filing' && !start) return path.dirname(path.dirname(SELF));
  let d = path.resolve(start || process.cwd());
  for (;;) {
    if (exists(path.join(d, '.filing', 'config.json'))) return d;
    const up = path.dirname(d);
    if (up === d) return path.resolve(start || process.cwd());
    d = up;
  }
}
export function loadConfig(root) {
  const raw = readText(path.join(root, '.filing', 'config.json'));
  const c = raw ? JSON.parse(raw) : {};
  return { ...DEFAULTS, ...c, layout: { ...DEFAULTS.layout, ...(c.layout || {}) } };
}
const INTERNAL = /^\.filing\/(journal|trash)\/|^\.filing\/lease|\.tmp-\d+-[0-9a-f]+$/;

/** Every non-ignored file (POSIX-relative). Git repos: `git ls-files`; otherwise a walk honouring .gitignore. */
export function listFiles(root, gitRepo) {
  if (gitRepo) {
    const all = git(root, ['ls-files', '-co', '--exclude-standard', '-z']).out.split('\0').filter(Boolean);
    const del = new Set(git(root, ['ls-files', '-d', '-z']).out.split('\0').filter(Boolean));
    return [...new Set(all)].filter((f) => !del.has(f) && !INTERNAL.test(f)).sort();
  }
  const ign = gitignoreMatcher(readText(path.join(root, '.gitignore')));
  const out = [];
  const walk = (dirRel) => {
    let ents; try { ents = fs.readdirSync(dirRel ? absOf(root, dirRel) : root, { withFileTypes: true }); } catch { return; }
    for (const e of ents) {
      const rel = dirRel ? `${dirRel}/${e.name}` : e.name;
      if (e.name === '.git' || INTERNAL.test(rel + (e.isDirectory() ? '/' : ''))) continue;
      if (e.isDirectory()) { if (!ign(rel, true) && e.name !== 'node_modules') walk(rel); }
      else if (e.isFile() && !ign(rel, false)) out.push(rel);
    }
  };
  walk('');
  return out.sort();
}

function context(root) {
  const cfg = loadConfig(root);
  const gitRepo = isGit(root);
  const files = listFiles(root, gitRepo);
  return { root, cfg, git: gitRepo, files, fileSet: new Set(files) };
}

// ---------------------------------------------------------------- lease lock
/**
 * Lease = .filing/lease.json {holder, cmd, pid, host, expires}. Taken by exclusive create.
 * A dead or expired lease is taken over by an atomic rename (one winner). Release deletes the file;
 * where deleting is forbidden it overwrites it with {released:true} and later takers claim it by
 * write-then-verify, so nothing ever needs deleting. Polls every 50 ms.
 */
const noDelete = () => process.env.FILING_TEST_NODELETE === '1';
const leaseFs = {
  unlink: (p) => { if (noDelete()) throw Object.assign(new Error('EPERM (test)'), { code: 'EPERM' }); fs.unlinkSync(p); },
  rename: (a, b) => { if (noDelete()) throw Object.assign(new Error('EPERM (test)'), { code: 'EPERM' }); fs.renameSync(a, b); },
};
function alive(pid) { try { process.kill(pid, 0); return true; } catch (e) { return e.code === 'EPERM'; } }
function leaseDead(l) {
  return !l || l.released || l.expires < Date.now() || (l.host === os.hostname() && !alive(l.pid));
}
export function acquire(root, cmd, waitMs = 30000) {
  const f = path.join(root, '.filing', 'lease.json');
  fs.mkdirSync(path.dirname(f), { recursive: true });
  const me = { holder: who(), cmd, pid: process.pid, host: os.hostname(), nonce: rand(6) };
  const body = () => JSON.stringify({ ...me, expires: Date.now() + LEASE_TTL });
  const start = Date.now(); let told = false;
  for (;;) {
    try {
      const fd = fs.openSync(f, 'wx'); fs.writeSync(fd, body()); fs.closeSync(fd);
      return { f, me, mode: 'file', renew: () => fs.writeFileSync(f, body()) };
    } catch (e) { if (e.code !== 'EEXIST') throw e; }
    let cur = null; const raw = readText(f);
    try { cur = JSON.parse(raw); } catch { /* being written, or junk */ }
    if (!cur) { let age = 0; try { age = Date.now() - fs.statSync(f).mtimeMs; } catch { continue; } if (age < 1000) { sleep(POLL_MS); continue; } }
    if (leaseDead(cur)) {
      if (!(cur && cur.released)) {
        const tomb = `${f}.stale.${me.nonce}`;
        let renamed = false;
        try { leaseFs.rename(f, tomb); renamed = true; } catch (e) { if (e.code === 'ENOENT') continue; }
        if (renamed) {
          if (readText(tomb) !== raw) { // we grabbed a lease created after our read: put it back without clobbering
            try { fs.linkSync(tomb, f); } catch (e) { if (e.code !== 'EEXIST' && !exists(f)) { try { fs.renameSync(tomb, f); } catch { /* lost */ } } }
          }
          try { leaseFs.unlink(tomb); } catch { /* left behind only where deleting is forbidden */ }
          continue;
        }
      }
      // Delete-forbidden place: claim by write, wait a beat, verify we are still the writer.
      try { fs.writeFileSync(f, body()); } catch { sleep(POLL_MS); continue; }
      sleep(25);
      try { if (JSON.parse(readText(f)).nonce === me.nonce) return { f, me, mode: 'content', renew: () => fs.writeFileSync(f, body()) }; } catch { /* lost */ }
      continue;
    }
    if (Date.now() - start > waitMs) throw new Fail(`busy: ${cur.holder} is running "${cur.cmd}" (pid ${cur.pid} on ${cur.host}). Try again.`);
    if (!told) { process.stderr.write(`waiting: lease held by ${cur.holder} (${cur.cmd})\n`); told = true; }
    sleep(POLL_MS);
  }
}
export function release(lease) {
  if (!lease) return;
  try { if (JSON.parse(readText(lease.f)).nonce !== lease.me.nonce) return; } catch { return; }
  if (lease.mode === 'file') { try { leaseFs.unlink(lease.f); return; } catch { /* fall through */ } }
  try { fs.writeFileSync(lease.f, JSON.stringify({ released: true, expires: 0 })); } catch { /* nothing else to do */ }
}
function withLease(root, cmd, fn) {
  const l = acquire(root, cmd);
  const onExit = () => release(l);
  process.on('exit', onExit);
  try { return fn(l); } finally { release(l); process.off('exit', onExit); }
}

// ---------------------------------------------------------------- ledger (append-only JSONL, folded)
const ledgerPath = (root) => path.join(root, '.filing', 'ledger.jsonl');
export function readLedger(root) {
  const raw = readText(ledgerPath(root)) || '';
  const events = []; const errors = [];
  raw.split(/\r?\n/).forEach((line, i) => {
    if (!line.trim()) return;
    try { events.push({ ...JSON.parse(line), _i: i }); } catch { errors.push(`ledger line ${i + 1} is not JSON`); }
  });
  events.sort((a, b) => (a.ts < b.ts ? -1 : a.ts > b.ts ? 1 : a._i - b._i));
  return { events, errors };
}
function appendEvent(root, ev) {
  const line = JSON.stringify({ ts: new Date().toISOString(), who: who(), ...ev });
  fs.mkdirSync(path.join(root, '.filing'), { recursive: true });
  fs.appendFileSync(ledgerPath(root), `${line}\n`);
}
export function fold({ events, errors }) {
  const items = new Map(); const closes = []; const errs = [...errors]; const lessons = new Map();
  for (const e of events) {
    if (e.op === 'lesson') { lessons.set(e.id, { id: e.id, wrong: e.wrong, instead: e.instead, ts: e.ts, by: e.who, retired: false }); continue; }
    if (e.op === 'unlesson') { const l = lessons.get(e.id); if (l) { l.retired = true; l.why = e.why || ''; } else errs.push(`retire for unknown lesson ${e.id}`); continue; }
    if (e.op === 'close') { closes.push(e); continue; }
    if (e.op === 'archive') continue;
    if (e.op === 'add') {
      if (items.has(e.id)) { errs.push(`duplicate item id ${e.id} (added twice, e.g. on two branches); rename one with "item set"`); continue; }
      items.set(e.id, { id: e.id, title: e.title, home: e.home || '', pkg: e.pkg || '', files: e.files || [], owner: e.owner || '',
        status: 'open', since: e.ts, by: e.who, notes: [], why: '' });
      continue;
    }
    const it = items.get(e.id);
    if (!it) { errs.push(`event "${e.op}" for unknown item ${e.id}`); continue; }
    if (e.op === 'start') { it.status = 'doing'; it.since = e.ts; it.by = e.who; }
    else if (e.op === 'done') { it.status = 'done'; it.since = e.ts; it.by = e.who; }
    else if (e.op === 'drop') { if (!e.why) errs.push(`item ${e.id} dropped without --why`); it.status = 'dropped'; it.why = e.why; it.since = e.ts; }
    else if (e.op === 'reopen') { it.status = 'open'; it.since = e.ts; }
    else if (e.op === 'note') it.notes.push({ ts: e.ts, who: e.who, text: e.text });
    else if (e.op === 'set') for (const k of ['title', 'home', 'pkg', 'files', 'owner']) if (k in e) it[k] = e[k];
  }
  return { items, closes, errors: errs, lessons: [...lessons.values()] };
}

// ---------------------------------------------------------------- docs: dates, summaries, references
function frontmatter(text) {
  const m = /^---\r?\n([\s\S]*?)\r?\n---/.exec(text || '');
  const o = {};
  if (m) for (const l of m[1].split(/\r?\n/)) { const kv = /^([\w-]+)\s*:\s*(.*)$/.exec(l); if (kv) o[kv[1].toLowerCase()] = kv[2].replace(/^["']|["']$/g, ''); }
  return o;
}
/** The date a doc declares itself true: frontmatter updated/last_updated/date, or a "Last updated:" line. */
export function declaredDate(text) {
  const fm = frontmatter(text);
  for (const k of ['updated', 'last_updated', 'last-updated', 'last_true', 'date']) { const d = DATE_RE.exec(fm[k] || ''); if (d) return d[1]; }
  const head = (text || '').split(/\r?\n/).slice(0, 40).join('\n');
  const m = /^[\s>*_-]*(?:last[ -]updated|updated|last true|as of)[*_]*\s*[:\-–][*_\s]*(\d{4}-\d{2}-\d{2})/im.exec(head);
  return m ? m[1] : null;
}
function summaryOf(text) {
  const fm = frontmatter(text);
  if (fm.summary || fm.description) return (fm.summary || fm.description).slice(0, 100);
  const body = (text || '').replace(/^---\r?\n[\s\S]*?\r?\n---/, '');
  for (const l of body.split(/\r?\n/)) {
    const t = l.replace(/^#+\s*/, '').trim();
    if (t && !t.startsWith('<!--')) return t.replace(/\|/g, '/').slice(0, 100);
  }
  return '';
}

/** Split markdown into prose vs code (fenced blocks and inline code) so links are only read from prose. */
function mdSegments(text) {
  const segs = []; const re = /(^|\n)(```|~~~)[^\n]*\n[\s\S]*?(\n\2[^\n]*(?=\n|$)|$)|`[^`\n]+`/g;
  let last = 0; let m;
  while ((m = re.exec(text))) { if (m.index > last) segs.push({ code: false, s: text.slice(last, m.index) }); segs.push({ code: true, s: m[0] }); last = m.index + m[0].length; }
  if (last < text.length) segs.push({ code: false, s: text.slice(last) });
  return segs;
}
const LINK_RE = /(!?\[[^\]\n]*\]\(\s*)(<[^>\n]+>|[^)\s]+)((?:\s+"[^"\n]*")?\s*\))|(^[ \t]{0,3}\[[^\]\n]+\]:[ \t]*)(<[^>\n]+>|\S+)/gm;
const isExternal = (t) => /^[a-z][a-z0-9+.-]*:/i.test(t) || t.startsWith('#') || t.startsWith('/') || t.startsWith('//');
function splitTarget(raw) {
  const angle = raw.startsWith('<') && raw.endsWith('>');
  const t = angle ? raw.slice(1, -1) : raw;
  const k = t.search(/[#?]/);
  const p = k < 0 ? t : t.slice(0, k);
  let dec = p; try { dec = decodeURIComponent(p); } catch { /* keep raw */ }
  return { angle, pathPart: p, decoded: dec, suffix: k < 0 ? '' : t.slice(k), encoded: /%[0-9a-f]{2}/i.test(p) };
}
/** Relative markdown links in a doc: [{target (decoded, as written), line}] */
export function mdLinks(text) {
  const out = [];
  for (const seg of mdSegments(text)) {
    if (seg.code) continue;
    for (const m of seg.s.matchAll(LINK_RE)) {
      const raw = m[2] || m[5]; if (isExternal(raw.replace(/^</, ''))) continue;
      const t = splitTarget(raw); if (!t.decoded) continue;
      out.push(t.decoded);
    }
  }
  return out;
}
const PATHY_EXT = /\.(ps1|psm1|py|sh|bash|js|mjs|cjs|ts|bat|cmd|json|ya?ml|toml|xml|sql|ini|cfg|md|go|rs|rb)$/i;
/** Path-looking tokens inside code spans and fenced blocks (commands that name scripts). */
export function codePaths(text) {
  const out = new Set();
  for (const seg of mdSegments(text)) {
    if (!seg.code) continue;
    for (let tok of seg.s.replace(/^(```|~~~)[^\n]*|`/g, ' ').split(/[\s"'=(),;|<>]+/)) {
      tok = tok.replace(/\\/g, '/').replace(/^\.\//, '').replace(/[:.,]+$/, '');
      if (tok && PATHY_EXT.test(tok) && !/^[a-z][a-z0-9+.-]*:\/\//i.test(tok) && !/^[A-Za-z]:\//.test(tok) && !tok.startsWith('/') && !tok.includes('*')) out.add(tok);
    }
  }
  return [...out];
}

function gitLastChange(root, rels) {
  const map = new Map();
  for (let i = 0; i < rels.length; i += 150) {
    const chunk = rels.slice(i, i + 150);
    const r = git(root, ['log', '--format=@@%ct %cs', '--name-only', '--', ...chunk.map((p) => `:(literal)${p}`)]);
    let cur = null;
    for (const line of r.out.split('\n')) {
      if (line.startsWith('@@')) { const [t, d] = line.slice(2).split(' '); cur = { t: +t * 1000, day: d }; }
      else if (line && cur && !map.has(line)) map.set(line, cur);
    }
  }
  return map;
}
let deletedCache = null;
function everDeleted(root, gitRepo, cfgRoot) {
  if (deletedCache && deletedCache.root === root) return deletedCache.set;
  let set;
  if (gitRepo) set = new Set(git(root, ['log', '--all', '--no-renames', '--diff-filter=D', '--name-only', '--format=']).out.split('\n').filter(Boolean));
  else { set = new Set(); for (const l of (readText(path.join(cfgRoot, '.filing', 'manifest.tsv')) || '').split('\n').slice(1)) if (l) set.add(l.split('\t')[0]); }
  deletedCache = { root, set };
  return set;
}

function resolveRef(ctx, docRel, target) {
  const t = target.replace(/\\/g, '/');
  const a = P.normalize(P.join(P.dirname(docRel), t));
  const b = P.normalize(t);
  for (const c of [a, b]) if (!c.startsWith('../') && (ctx.fileSet.has(c) || ctx.dirSet.has(c.replace(/\/$/, '')))) return c;
  return null;
}
function dirSetOf(files) { const s = new Set(); for (const f of files) { let d = P.dirname(f); while (d !== '.' && !s.has(d)) { s.add(d); d = P.dirname(d); } } return s; }

/**
 * Doc analysis: broken links (FAIL), and STALE — a referenced file changed (committed) after the doc's
 * last-true date, or a referenced file that existed once and is now gone. Uncommitted edits never count.
 */
export function analyzeDocs(ctx, docs, opts = {}) {
  ctx.dirSet = ctx.dirSet || dirSetOf(ctx.files);
  const fail = []; const warn = [];
  const texts = new Map(); const refsOf = new Map(); const allRefs = new Set();
  for (const d of docs) {
    const text = readText(absOf(ctx.root, d)); if (text == null) continue;
    texts.set(d, text);
    const refs = new Set();
    for (const l of mdLinks(text)) {
      const r = resolveRef(ctx, d, l);
      if (r) { if (ctx.fileSet.has(r) && r !== d) refs.add(r); } else fail.push({ code: 'BROKEN-LINK', path: d, msg: `link to missing "${l}"` });
    }
    const gone = [];
    for (const c of codePaths(text)) {
      const r = resolveRef(ctx, d, c);
      if (r) { if (ctx.fileSet.has(r) && r !== d) refs.add(r); } else gone.push(c);
    }
    refsOf.set(d, { refs: [...refs], gone });
    refs.forEach((r) => allRefs.add(r));
  }
  const changed = ctx.git ? gitLastChange(ctx.root, [...new Set([...allRefs, ...texts.keys()])]) : null;
  const when = (rel) => {
    if (changed) return changed.get(rel) || null;
    try { const st = fs.statSync(absOf(ctx.root, rel)); return { t: st.mtimeMs, day: today(new Date(st.mtimeMs)) }; } catch { return null; }
  };
  let deleted = null;
  for (const [d, text] of texts) {
    const declared = declaredDate(text);
    const own = when(d);
    const { refs, gone } = refsOf.get(d);
    for (const g of gone) {
      const cands = [P.normalize(P.join(P.dirname(d), g)), P.normalize(g)];
      deleted = deleted || everDeleted(ctx.root, ctx.git, ctx.root);
      if (cands.some((c) => deleted.has(c))) warn.push({ code: 'STALE', path: d, msg: `references "${g}", which no longer exists` });
    }
    for (const r of refs) {
      const rc = when(r); if (!rc) continue;
      const newer = declared ? rc.day > declared : own && rc.t > own.t;
      if (newer) warn.push({ code: 'STALE', path: d, msg: `"${r}" changed ${rc.day}, after this doc was last true (${declared || (own && own.day)})` });
    }
    if (declared && own && ctx.git && own.day > declared && !opts.noEdited) warn.push({ code: 'EDITED', path: d, msg: `committed ${own.day} but still says it was true on ${declared}; re-date it if it is still true` });
  }
  return { fail, warn, texts };
}

/** Docs that INDEX.md lists. */
export function docSet(ctx) {
  const L = ctx.cfg.layout;
  const gen = new Set([L.handoff, L.index]);
  return ctx.files.filter((f) => /\.(md|markdown)$/i.test(f) && anyGlob(ctx.cfg.docGlobs, f) && !anyGlob(ctx.cfg.docExclude, f)
    && !gen.has(f) && !FRONT_DOORS.has(P.basename(f).toLowerCase()) && !f.startsWith('.filing/'));
}

// ---------------------------------------------------------------- facts: evidence for the card (schedules, config, outputs)
const EXEC_EXT = new Set(['.sh', '.bash', '.zsh', '.py', '.ps1', '.psm1', '.bat', '.cmd', '.js', '.mjs', '.cjs', '.ts', '.rb', '.pl', '.php', '.exe']);
const SYS_PREFIX = /^\/(usr|bin|sbin|etc|dev|proc|sys|lib|lib64|tmp|run|var\/run|var\/lib|var\/tmp|nix|snap)(\/|$)/i;
const OUTPUT_NAMES = new Set(['out', 'output', 'outputs', 'exports', 'export', 'logs', 'log', 'reports', 'report', 'results', 'backups', 'backup', 'data', 'artifacts']);
const DOW = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const DOW3 = DOW.map((d) => d.slice(0, 3).toLowerCase());
const RUNBOOK = /runbook|playbook|restore|recover|disaster|incident|rollback|rotat|troubleshoot|how-?to|procedure|emergency|on-?call/i;
const CRON_FIELD = /^(?:[\d*][\d*/,-]*|[A-Za-z]{3}(?:[,-][A-Za-z]{3})*)$/;
const pad2 = (x) => String(x).padStart(2, '0');

/** Text of a file in any encoding a scheduler writes (UTF-16 LE/BE with or without BOM, UTF-8 with BOM). */
export function decodeText(buf) {
  if (buf.length >= 2 && buf[0] === 0xff && buf[1] === 0xfe) return buf.subarray(2).toString('utf16le');
  if (buf.length >= 2 && buf[0] === 0xfe && buf[1] === 0xff) { const b = Buffer.from(buf.subarray(2)); b.swap16(); return b.toString('utf16le'); }
  if (buf.length >= 4 && buf[1] === 0 && buf[3] === 0 && buf[0] !== 0) return buf.toString('utf16le');
  return buf.toString('utf8').replace(/^﻿/, '');
}
function dowName(f) {
  const one = (x) => { const n = /^\d$/.test(x) ? +x % 7 : DOW3.indexOf(x.toLowerCase()); return n >= 0 ? DOW[n] : x; };
  return f.split(',').map((part) => (part.includes('-') ? part.split('-').map(one).join('–') : one(part))).join(', ');
}
/** One cron expression in plain words ("daily 02:15", "Sunday 03:30", "every 15 min"). Unusual forms are returned raw. */
export function describeCron(expr) {
  const named = { '@hourly': 'hourly', '@daily': 'daily 00:00', '@midnight': 'daily 00:00', '@weekly': 'Sunday 00:00 (weekly)', '@monthly': 'monthly, day 1 00:00', '@yearly': 'yearly', '@annually': 'yearly', '@reboot': 'at boot' };
  if (named[expr]) return named[expr];
  const [mi, h, dom, mon, dow] = expr.split(/\s+/);
  const step = (f) => /^\*\/(\d+)$/.exec(f);
  let when; let periodic = false;
  if (mi === '*' && h === '*') { when = 'every minute'; periodic = true; }
  else if (step(mi) && h === '*') { when = `every ${step(mi)[1]} min`; periodic = true; }
  else if (/^\d+$/.test(mi) && h === '*') { when = `hourly at :${pad2(mi)}`; periodic = true; }
  else if (/^\d+$/.test(mi) && step(h)) { when = `every ${step(h)[1]} h at :${pad2(mi)}`; periodic = true; }
  else if (/^\d+$/.test(mi) && /^\d+(,\d+)*$/.test(h)) when = h.split(',').map((x) => `${pad2(x)}:${pad2(mi)}`).join(', ');
  else return `cron \`${expr}\``;
  if (mon !== '*') return `${when}, cron \`${expr}\``;
  if (dom === '*' && dow === '*') return periodic ? when : `daily ${when}`;
  if (dom === '*') return `${dowName(dow)} ${when}`;
  if (dow === '*') return `monthly on day ${dom}, ${when}`;
  return `${when}, cron \`${expr}\``;
}
/** A crontab line → {expr, cmd, retired} (retired = commented out but still a valid schedule line), or null. */
export function parseCronLine(line, hasUser = false) {
  let t = line.trim(); let retired = false;
  if (t.startsWith('#')) { t = t.replace(/^#+\s*/, ''); retired = true; }
  if (!t || /^[A-Za-z_][A-Za-z0-9_]*=/.test(t)) return null;
  let m = /^(@(?:reboot|yearly|annually|monthly|weekly|daily|midnight|hourly))\s+(.+)$/.exec(t);
  if (m) return { expr: m[1], cmd: hasUser ? m[2].replace(/^\S+\s+/, '') : m[2], retired };
  const parts = t.split(/\s+/);
  if (parts.length < 6 || !parts.slice(0, 5).every((f) => CRON_FIELD.test(f))) return null;
  if (!/[\d*]/.test(parts.slice(0, 2).join(''))) return null;
  return { expr: parts.slice(0, 5).join(' '), cmd: t.replace(hasUser ? /^(\S+\s+){6}/ : /^(\S+\s+){5}/, '').trim(), retired };
}
const xmlTag = (x, name) => { const m = new RegExp(`<${name}\\b[^>]*>([\\s\\S]*?)</${name}>`, 'i').exec(x || ''); return m ? m[1].trim() : null; };
const xmlText = (s) => (s || '').replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');
function isoDuration(d) { const m = /^P(?:(\d+)D)?T?(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?$/.exec(d || ''); if (!m) return d; const [, D, H, M] = m; return D ? `${D} day(s)` : H ? `${H} h` : M ? `${M} min` : d; }
/** Windows Task Scheduler XML → {name, when[], actions[{command, args, cwd}], disabled}, or null. */
export function parseTaskXml(text) {
  if (!/<Task\b/.test(text) || !/<Actions\b/.test(text)) return null;
  const name = xmlText(xmlTag(text, 'URI')) || '';
  const when = [];
  for (const m of (xmlTag(text, 'Triggers') || '').matchAll(/<(CalendarTrigger|TimeTrigger|BootTrigger|LogonTrigger|IdleTrigger|EventTrigger|RegistrationTrigger|SessionStateChangeTrigger)\b[^>]*>([\s\S]*?)<\/\1>/g)) {
    const [, kind, body] = m;
    const tm = /T(\d{2}:\d{2})/.exec(xmlTag(body, 'StartBoundary') || ''); const at = tm ? tm[1] : '';
    let w;
    if (kind === 'CalendarTrigger') {
      const byDay = xmlTag(body, 'ScheduleByDay'); const byWeek = xmlTag(body, 'ScheduleByWeek'); const byMonth = xmlTag(body, 'ScheduleByMonth');
      if (byWeek != null) {
        const days = [...(xmlTag(byWeek, 'DaysOfWeek') || '').matchAll(/<(\w+)\s*\/>/g)].map((x) => x[1]);
        const n = +(xmlTag(byWeek, 'WeeksInterval') || 1);
        w = `${days.join(', ') || 'weekly'} ${at}${n > 1 ? ` (every ${n} weeks)` : ''}`.trim();
      } else if (byMonth != null) {
        const days = [...(xmlTag(byMonth, 'DaysOfMonth') || '').matchAll(/<Day>(\w+)<\/Day>/g)].map((x) => x[1]);
        w = `monthly on day ${days.join(', ') || '?'} ${at}`.trim();
      } else if (byDay != null) { const n = +(xmlTag(byDay, 'DaysInterval') || 1); w = n > 1 ? `every ${n} days ${at}` : `daily ${at}`; }
      else w = `calendar ${at}`;
    } else if (kind === 'TimeTrigger') w = `once at ${(xmlTag(body, 'StartBoundary') || '').replace('T', ' ')}`;
    else w = { BootTrigger: 'at boot', LogonTrigger: 'at logon', IdleTrigger: 'when idle', EventTrigger: 'on an event', RegistrationTrigger: 'when registered', SessionStateChangeTrigger: 'on session change' }[kind];
    const rep = xmlTag(xmlTag(body, 'Repetition'), 'Interval'); if (rep) w += `, repeating every ${isoDuration(rep)}`;
    if (/^false$/i.test(xmlTag(body, 'Enabled') || '')) w += ' (trigger disabled)';
    when.push(w.trim());
  }
  const actions = [...(xmlTag(text, 'Actions') || '').matchAll(/<Exec>([\s\S]*?)<\/Exec>/g)].map((m) => ({ command: xmlText(xmlTag(m[1], 'Command')), args: xmlText(xmlTag(m[1], 'Arguments')), cwd: xmlText(xmlTag(m[1], 'WorkingDirectory')) }));
  return { name, when, actions, disabled: /^false$/i.test(xmlTag(xmlTag(text, 'Settings'), 'Enabled') || '') };
}
/** systemd OnCalendar value in plain words. */
export function describeOnCalendar(v) {
  const t = v.trim(); const named = { minutely: 'every minute', hourly: 'hourly', daily: 'daily 00:00', weekly: 'Monday 00:00 (weekly)', monthly: 'monthly, day 1 00:00' };
  if (named[t.toLowerCase()]) return named[t.toLowerCase()];
  const m = /^(?:([A-Za-z,.-]+)\s+)?\*-\*-\*\s+(\d{1,2}):(\d{2})(?::\d{2})?$/.exec(t);
  const day = (d) => DOW[DOW3.indexOf(d.slice(0, 3).toLowerCase())] || d;
  if (m) { const days = m[1] ? m[1].split(',').map((d) => d.split('..').map(day).join('–')).join(', ') : ''; return `${days || 'daily'} ${pad2(m[2])}:${m[3]}`; }
  return `OnCalendar \`${t}\``;
}
function cmdTokens(cmd) {
  const toks = []; const re = /"([^"]*)"|'([^']*)'|(\S+)/g; let m;
  while ((m = re.exec(cmd))) toks.push(m[1] ?? m[2] ?? m[3]);
  return toks;
}
/** Split a command into its redirect targets (logs) and the rest. */
function splitRedirects(cmd) {
  const logs = []; const rest = cmd.replace(/\s*\d?>>?\s*(&\d|"[^"]*"|'[^']*'|\S+)/g, (w, t) => { if (!t.startsWith('&')) logs.push(t.replace(/^["']|["']$/g, '')); return ' '; });
  return { rest: rest.trim(), logs };
}
/**
 * Map a path as written in a scheduler or script (absolute, deployed elsewhere, Windows or POSIX) onto a repo
 * file by the longest matching path suffix. Learns deployment roots ("/srv/app" → repo root) as it goes.
 */
function makeMapper(ctx) {
  const roots = new Set(); const lowFiles = ctx.files.map((f) => [f.toLowerCase(), f]);
  const norm = (tok) => tok.replace(/\\/g, '/').replace(/^["']|["']$/g, '').replace(/^\.\//, '');
  const map = (tok0) => {
    const tok = norm(tok0); if (!tok || tok.length < 3) return null;
    if (ctx.fileSet.has(tok)) return { rel: tok, exists: true };
    const low = tok.toLowerCase();
    if (SYS_PREFIX.test(tok)) return null;
    let best = null;
    for (const [lf2, f] of lowFiles) if (low.endsWith(`/${lf2}`) && (!best || f.length > best.length)) best = f;
    if (best && (best.includes('/') || /^(\/|[a-z]:\/|\$|%|~)/i.test(tok))) { roots.add(low.slice(0, low.length - best.length - 1)); return { rel: best, exists: true }; }
    for (const r of roots) if (low.startsWith(`${r}/`)) return { rel: tok.slice(r.length + 1), exists: ctx.fileSet.has(tok.slice(r.length + 1)) };
    return null;
  };
  map.roots = roots; map.norm = norm;
  return map;
}
const CODE_COMMENT = /^\s*(#|REM\b|::|\/\/|<#|;)/i;
const SOURCE_LINE = /^\s*(source\s|\.\s+["'$\w./\\]|import\s|from\s+\S+\s+import\b|Import-Module\b|require\(|const\s+\w+\s*=\s*require\(|using\s)/i;
const ECHO_LINE = /^\s*(echo|printf|print\(|Write-(Host|Output|Verbose|Warning|Information)\b|logger\b|console\.log)/i;
function findToken(line, form, ci) {
  const L = ci ? line.toLowerCase() : line; const F = ci ? form.toLowerCase() : form;
  for (let k = L.indexOf(F); k >= 0; k = L.indexOf(F, k + 1)) {
    const before = k === 0 ? ' ' : L[k - 1]; const after = L[k + F.length] ?? ' ';
    if (/[\s"'/=(`]/.test(before) && /[\s"'`);&|]/.test(after)) return k;
  }
  return -1;
}
/** The repo scripts a script invokes, in the order it invokes them (sourced helpers and imports excluded). */
export function scriptSteps(ctx, rel) {
  const buf = (() => { try { return fs.readFileSync(absOf(ctx.root, rel)); } catch { return null; } })();
  if (!buf || buf.length > 1 << 20) return [];
  const ci = /\.(ps1|psm1|bat|cmd)$/i.test(rel);
  const cands = ctx.files.filter((f) => f !== rel && EXEC_EXT.has(P.extname(f).toLowerCase()) && !SECRET_NAME.test(P.basename(f)) && !f.startsWith('.filing/'));
  const dir = P.dirname(rel); const steps = [];
  for (const raw of decodeText(buf).split(/\r?\n/)) {
    if (CODE_COMMENT.test(raw) || SOURCE_LINE.test(raw) || ECHO_LINE.test(raw)) continue;
    const line = raw.replace(/\\/g, '/').replace(/\s#\s.*$/, '');
    const hits = [];
    for (const f of cands) {
      const forms = [f]; const r = P.relative(dir, f); if (r !== f) forms.push(r);
      for (const form of forms) { const k = findToken(line, form, ci); if (k >= 0) { hits.push([k, f]); break; } }
    }
    hits.sort((a, b) => a[0] - b[0]);
    for (const [, f] of hits) if (!steps.includes(f)) steps.push(f);
  }
  return steps;
}
/** Absolute and network paths a script names (outside the repo once deployment roots are mapped). */
function namedPaths(ctx, rel, map) {
  const t = (() => { try { return decodeText(fs.readFileSync(absOf(ctx.root, rel))); } catch { return ''; } })();
  const ext = []; const repoDirs = new Set();
  for (const raw of t.split(/\r?\n/)) {
    if (CODE_COMMENT.test(raw) || SOURCE_LINE.test(raw)) continue;
    for (const m of raw.matchAll(/\\\\[\w.$-]+\\[^\s"'`;|]*/g)) ext.push(m[0].endsWith('\\') ? m[0] : `${m[0].replace(/\\[^\\]*\.\w+$/, '')}\\`);
    for (const m of raw.matchAll(/(?:^|[\s"'=(])((?:\/[\w.@+-]+){2,}\/?)(.?)/g)) {
      let p2 = m[1]; if (SYS_PREFIX.test(p2)) continue;
      if (m[2] && !/[\s"'`);&|]/.test(m[2])) p2 = `${P.dirname(p2)}/`; // stops at a $(…) or variable: keep the folder
      else if (/\.\w{1,5}$/.test(p2)) p2 = `${P.dirname(p2)}/`;
      if (!p2.endsWith('/')) p2 += '/';
      const mapped = map(p2.slice(0, -1)) || map(`${p2}x`);
      if (mapped) { const top = mapped.rel.split('/')[0]; if (ctx.dirSet.has(top) && OUTPUT_NAMES.has(top.toLowerCase())) repoDirs.add(top); continue; }
      ext.push(p2);
    }
    for (const m of raw.matchAll(/(?:\$\{?\w+\}?|%\w+%|\$PSScriptRoot)[/\\](?:\.\.[/\\])*([\w .-]+?)[/\\]/g)) if (ctx.dirSet.has(m[1]) && (OUTPUT_NAMES.has(m[1].toLowerCase()) || /[^-]>|Out-File|Set-Content|Add-Content|-Destination|\btee\b|--out\b/i.test(raw))) repoDirs.add(m[1]);
  }
  return { external: [...new Set(ext)], repoDirs: [...repoDirs] };
}
function isConfigFile(f) {
  const b = P.basename(f).toLowerCase(); const top = f.split('/')[0].toLowerCase();
  if (!/\.(json|ya?ml|toml|ini|cfg|conf|properties)$/.test(b) || SECRET_NAME.test(b) || f.startsWith('.filing/') || f.startsWith('.')) return false;
  if (/^(package(-lock)?|tsconfig.*|jsconfig|composer|bower|lerna|turbo|nx|renovate|\.?eslintrc.*|\.?prettierrc.*|pnpm-lock|yarn|manifest|launch|extensions|settings\.local)\.(json|ya?ml)$/.test(b)) return false;
  return ['config', 'configs', 'conf', 'settings', 'cfg', 'etc'].includes(top) || /^(settings|config|configuration|appsettings|application|app)([._-][\w.-]*)?\.\w+$/.test(b);
}
function purposeOf(text) {
  const t = text || ''; const p = /^\s*(?:purpose|summary|use (?:this )?when)\s*[:-]\s*(.+)$/im.exec(t);
  if (p) return p[1].trim().slice(0, 100);
  const lines = t.replace(/^---\r?\n[\s\S]*?\r?\n---/, '').split(/\r?\n/).map((l) => l.trim()).filter((l) => l && !l.startsWith('<!--'));
  const pick = lines.find((l) => /^#+\s/.test(l)) || lines.find((l) => l.split(/\s+/).length >= 3) || lines[0] || '';
  return pick.replace(/^#+\s*/, '').replace(/[`*_|]/g, '').slice(0, 100);
}
/** Markdown links in a line from another doc become "text (`path`)", so the card never carries a link relative to some other folder. */
function delink(line, fromDoc, ctx) {
  return line.replace(/(!?)\[([^\]\n]*)\]\(\s*<?([^)>\s]+)>?(?:\s+"[^"]*")?\s*\)/g, (w, bang, text, target) => {
    if (isExternal(target)) return w;
    let t = target.replace(/[#?].*$/, ''); try { t = decodeURIComponent(t); } catch { /* raw */ }
    const r = resolveRef(ctx, fromDoc, t);
    return r ? `${text} (\`${r}\`)` : text;
  });
}
function readArchiveIndex(ctx) {
  const t = readText(absOf(ctx.root, `${ctx.cfg.layout.archive}/INDEX.tsv`)) || '';
  return t.split(/\r?\n/).slice(1).filter(Boolean).map((l) => { const [date, from, to, by, whoBy, hash] = l.split('\t'); return { date, from, to, by, who: whoBy, hash: hash || '' }; });
}

/** Everything the card states that can be read from the repo itself. Deterministic: no clock, no git history. */
export function facts(ctx) {
  ctx.dirSet = ctx.dirSet || dirSetOf(ctx.files);
  const map = makeMapper(ctx); const F = { schedules: [], retired: [], config: {}, outputs: [], external: [], runbooks: [], readme: {}, env: {}, archive: [], sources: [], limits: [] };
  const raws = [];
  // cron
  for (const f of ctx.files) {
    const b = P.basename(f).toLowerCase();
    if (!(/^crontab(\.[\w-]+)?$|\.cron$|\.crontab$/.test(b) || P.dirname(f).endsWith('cron.d'))) continue;
    const text = readText(absOf(ctx.root, f)); if (!text) continue; F.sources.push(f);
    for (const line of text.split(/\r?\n/)) { const c = parseCronLine(line, P.dirname(f).endsWith('cron.d')); if (c) raws.push({ kind: 'cron', source: f, when: describeCron(c.expr), expr: c.expr, cmd: c.cmd, retired: c.retired }); }
  }
  // Windows Task Scheduler XML
  for (const f of ctx.files) {
    if (!/\.xml$/i.test(f) || f.startsWith('.filing/')) continue;
    let buf; try { buf = fs.readFileSync(absOf(ctx.root, f)); } catch { continue; } if (buf.length > 1 << 20) continue;
    const task = parseTaskXml(decodeText(buf)); if (!task) continue; F.sources.push(f);
    for (const a of task.actions) raws.push({ kind: 'task', source: f, name: task.name, when: task.when.join('; ') || 'no trigger (run by hand)', cmd: `${/powershell|pwsh|cmd(\.exe)?$|python|node|bash/i.test(a.command) ? '' : `"${a.command}" `}${a.args || ''}`.trim() || a.command, exe: a.command, cwd: a.cwd, retired: task.disabled });
  }
  // systemd timers
  for (const f of ctx.files) {
    if (!/\.timer$/i.test(f)) continue;
    const t = readText(absOf(ctx.root, f)) || ''; F.sources.push(f);
    const whens = [...t.matchAll(/^\s*OnCalendar\s*=\s*(.+)$/gm)].map((m) => describeOnCalendar(m[1])).concat([...t.matchAll(/^\s*(OnBootSec|OnUnitActiveSec|OnStartupSec)\s*=\s*(.+)$/gm)].map((m) => `${m[1]}=${m[2].trim()}`));
    const unit = (/^\s*Unit\s*=\s*(\S+)/m.exec(t) || [])[1] || P.basename(f).replace(/\.timer$/i, '.service');
    const svc = ctx.files.find((x) => P.basename(x) === unit);
    const execs = svc ? [...(readText(absOf(ctx.root, svc)) || '').matchAll(/^\s*ExecStart\s*=\s*[-@+!:]*(.+)$/gm)].map((m) => m[1]) : [];
    if (svc) F.sources.push(svc);
    for (const cmd of execs.length ? execs : ['']) raws.push({ kind: 'systemd', source: svc ? `${f} + ${svc}` : f, when: whens.join('; ') || 'no OnCalendar', cmd, retired: false, unit });
  }
  // resolve targets: active entries first so deployment roots are learnt before mapping retired ones
  const resolve = (r) => {
    const { rest, logs } = splitRedirects(r.cmd);
    let target = null; let raw = '';
    for (const tok of cmdTokens(rest)) {
      if (/^-/.test(tok) && !/^-File$/i.test(tok)) continue;
      const m = map(tok); if (m) { target = m; raw = tok; break; }
    }
    if (!target) { const tok = cmdTokens(rest).find((x) => /[/\\]/.test(x) && !/^-/.test(x)); raw = tok || rest; }
    return { ...r, target: target ? target.rel : null, targetExists: !!(target && target.exists), raw, logs: logs.map((l) => { const m = map(l); return m ? m.rel : l; }) };
  };
  const active = raws.filter((r) => !r.retired).map(resolve);
  const retired = raws.filter((r) => r.retired).map(resolve);
  for (const s of active) {
    s.steps = s.target && s.targetExists ? scriptSteps(ctx, s.target) : [];
    const writes = new Set(); const repoDirs = new Set();
    for (const rel of s.target && s.targetExists ? [s.target, ...s.steps] : []) { const n = namedPaths(ctx, rel, map); n.external.forEach((x) => writes.add(x)); n.repoDirs.forEach((x) => repoDirs.add(x)); }
    s.writes = [...writes]; s.repoDirs = [...repoDirs];
    s.deployedAt = (() => { const tok = map.norm(s.raw).toLowerCase(); for (const r of map.roots) if (tok.startsWith(`${r}/`)) return s.raw.slice(0, r.length); return ''; })();
  }
  F.schedules = active;
  const docsText = ctx.files.filter((f) => /\.(md|markdown|txt)$/i.test(f) && !f.startsWith(`${ctx.cfg.layout.archive}/`) && !f.startsWith('.filing/') && !FRONT_DOORS.has(P.basename(f).toLowerCase()) && f !== ctx.cfg.layout.index);
  F.retired = retired.map((r) => {
    const base = r.target ? P.basename(r.target) : P.basename(map.norm(r.raw));
    const namedBy = base && base.length > 3 ? docsText.filter((d) => !F.sources.includes(d) && (readText(absOf(ctx.root, d)) || '').includes(base)) : [];
    return { ...r, namedBy };
  });
  // config: the file code reads most is the source of truth
  const cfgFiles = ctx.files.filter(isConfigFile);
  const codeFiles = ctx.files.filter((f) => (CODE_EXT.has(P.extname(f).toLowerCase()) || CODE_NAMES.test(P.basename(f))) && !cfgFiles.includes(f) && !SECRET_NAME.test(P.basename(f)) && !f.startsWith('.filing/') && !/\.xml$/i.test(f));
  const refCount = new Map(cfgFiles.map((c) => [c, []]));
  if (cfgFiles.length) {
    const bases = cfgFiles.map((c) => [c, P.basename(c).toLowerCase()]);
    for (const f of codeFiles) {
      let st; try { st = fs.statSync(absOf(ctx.root, f)); } catch { continue; } if (st.size > 512 << 10) continue;
      const t = (readText(absOf(ctx.root, f)) || '').toLowerCase();
      for (const [c, b] of bases) if (t.includes(b)) refCount.get(c).push(f);
    }
  }
  const ranked = [...refCount].sort((a, b) => b[1].length - a[1].length || (a[0] < b[0] ? -1 : 1));
  if (ranked.length && (ranked[0][1].length > 0 || ranked.length === 1)) {
    const [src, by] = ranked[0];
    const tie = ranked.filter(([, b]) => b.length === by.length && by.length > 0).map(([c]) => c);
    F.config.source = src; F.config.readBy = by.length; F.config.tie = tie.length > 1 ? tie : [];
    F.config.others = ranked.slice(1).map(([c, b]) => ({ path: c, readBy: b.length }));
    const text = readText(absOf(ctx.root, src)) || ''; const keys = [];
    const pathy = /(dir|path|folder|file|output|export|log|dest|db|database|backup|url|root)$/i;
    if (/\.json$/i.test(src)) { try { const j = JSON.parse(text); for (const [k, v] of Object.entries(j)) if (typeof v === 'string' && pathy.test(k)) keys.push([k, v]); } catch { F.limits.push(`${src} is not valid JSON; keys not read`); } }
    else for (const m of text.matchAll(/^([A-Za-z_][\w.-]*)\s*[:=]\s*["']?([^"'\n#]+?)["']?\s*$/gm)) if (pathy.test(m[1])) keys.push([m[1], m[2]]);
    F.config.keys = keys.slice(0, 8);
  }
  // secrets: names only. Gitignored ones count too (that is where they should be); contents are never read.
  F.config.secrets = [...new Set([...ctx.files.filter((f) => !f.includes('/') && /^\.env(\.[\w-]+)?$/i.test(f)), ...['.env', '.env.local', '.env.production'].filter((f) => isFile(path.join(ctx.root, f)))])].filter((f) => !/example|sample|template|dist/i.test(f)).sort();
  const envT = ctx.files.filter((f) => /^(\.?env\.(example|sample|template|dist)(\.[\w.-]+)?|example\.env)$/i.test(P.basename(f)) && !f.startsWith(`${ctx.cfg.layout.archive}/`));
  const plain = envT.filter((f) => !/\.(old|bak|orig|copy|backup|\d+)$/i.test(f) && !familyKey(P.basename(f).replace(/^\./, '')).marked).sort((a, b) => a.split('/').length - b.split('/').length || a.length - b.length);
  F.env = { template: plain[0] || null, others: envT.filter((f) => f !== plain[0]) };
  // README
  const readmes = ctx.files.filter((f) => !f.includes('/') && /^readme/i.test(f));
  const cur = readmes.find((f) => /^readme(\.md|\.markdown|\.txt|\.rst)?$/i.test(f)) || null;
  F.readme = { current: cur, others: readmes.filter((f) => f !== cur) };
  // outputs: path keys in config, folders written by scheduled scripts, conventional output folders the code names
  const outs = new Map(); const addOut = (p2, ev) => { const k = p2.replace(/\/+$/, ''); if (!k || k.startsWith('..')) return; if (!outs.has(k)) outs.set(k, new Set()); outs.get(k).add(ev); };
  for (const [k, v] of F.config.keys || []) { if (/^[a-z]+:\/\//i.test(v) || /^\//.test(v) || /\\\\/.test(v)) continue; const d = /\.\w{1,6}$/.test(v) ? P.dirname(v) : v; if (d && d !== '.') addOut(d.replace(/\\/g, '/'), `${P.basename(F.config.source)} ${k}`); }
  for (const s of F.schedules) {
    for (const d of s.repoDirs) addOut(d, s.target);
    for (const l of s.logs) if (!/^\//.test(l) && !/^[a-z]:/i.test(l) && P.dirname(l) !== '.') addOut(P.dirname(l), `${s.source} redirect`);
  }
  const schedTexts = F.schedules.flatMap((s) => (s.target && s.targetExists ? [s.target, ...s.steps] : []));
  for (const d of [...ctx.dirSet].filter((x) => !x.includes('/') && OUTPUT_NAMES.has(x.toLowerCase()))) {
    if (outs.has(d)) continue;
    const by = schedTexts.find((f) => { const t = readText(absOf(ctx.root, f)) || ''; return new RegExp(`[/\\\\"' ]${d.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}[/\\\\]`).test(t); });
    if (by) addOut(d, by);
  }
  F.outputs = [...outs].map(([p2, ev]) => ({ path: p2, evidence: [...ev].slice(0, 2) })).sort((a, b) => (a.path < b.path ? -1 : 1));
  const ext = new Map(); for (const s of F.schedules) for (const w of s.writes) { if (!ext.has(w)) ext.set(w, new Set()); ext.get(w).add(s.target); }
  F.external = [...ext].map(([p2, by]) => ({ path: p2, by: [...by] }));
  // runbooks
  F.runbooks = ctx.files.filter((f) => /\.(md|markdown|txt)$/i.test(f) && RUNBOOK.test(f) && !f.startsWith(`${ctx.cfg.layout.archive}/`) && !f.startsWith(`${ctx.cfg.layout.sessions}/`) && !f.startsWith('.filing/'))
    .map((f) => ({ path: f, purpose: purposeOf(readText(absOf(ctx.root, f))) }));
  F.archive = readArchiveIndex(ctx);
  const rowTo = new Set(F.archive.map((x) => x.to));
  F.archiveUnindexed = ctx.files.filter((f) => f.startsWith(`${ctx.cfg.layout.archive}/`) && f !== `${ctx.cfg.layout.archive}/INDEX.tsv` && !rowTo.has(f));
  F.limits.push('schedules are read from crontab files, cron.d, *.timer + *.service and Task Scheduler XML; schedules defined only on a machine (not in the repo) are invisible');
  return F;
}
export function factsMarkdown(F) {
  const L = ['# Facts (evidence read from the repo)', ''];
  L.push('## Schedules', '', ...(F.schedules.length ? F.schedules.flatMap(schedLines) : ['- none found']), ...(F.retired.length ? ['', ...F.retired.map(retiredLine)] : []), '');
  L.push('## Config', '', ...configLines(F), '');
  L.push('## Outputs', '', ...outputLines(F), '');
  L.push('## Runbooks', '', ...(F.runbooks.length ? F.runbooks.map((r) => `- \`${r.path}\` — ${r.purpose}`) : ['- none found']), '');
  L.push('## Limits', '', ...F.limits.map((x) => `- ${x}`), '');
  return L.join('\n');
}
const code = (p2) => `\`${p2}\``;
function schedLines(s) {
  const what = s.target ? `${code(s.target)}${s.targetExists ? '' : ' (**MISSING** from the repo)'}` : code(s.raw || '(no command)');
  const via = s.kind === 'cron' ? `cron \`${s.expr}\` in ${code(s.source)}` : s.kind === 'task' ? `Windows task ${s.name ? code(s.name) : ''} in ${code(s.source)}` : `systemd ${code(s.source)}`;
  const out = [`- **${s.when}** — ${what} (${via}${s.deployedAt ? `; deployed at ${code(s.deployedAt)}` : ''})`];
  if (s.steps && s.steps.length) out.push(`  - steps, in order: ${s.steps.map((x, i) => `${i + 1}. ${code(x)}`).join(' ')}`);
  const w = [...(s.logs || []).map((l) => `log ${code(l)}`), ...(s.writes || []).map((x) => code(x))];
  if (w.length) out.push(`  - writes: ${w.join(', ')}`);
  return out;
}
function retiredLine(r) {
  return `- Retired (commented out in ${code(r.source)}): ${r.when} ${code(r.target || r.raw)}${r.target && !r.targetExists ? ' — the file no longer exists' : ''}${r.namedBy.length ? `; docs still naming it: ${r.namedBy.map(code).join(', ')}` : ''}`;
}
function configLines(F) {
  const c = F.config; const out = [];
  if (c.source) out.push(`- Source of truth: ${code(c.source)} (read by ${c.readBy} code file${c.readBy === 1 ? '' : 's'})${c.tie && c.tie.length ? ` — **tie** with ${c.tie.filter((x) => x !== c.source).map(code).join(', ')}: the owner decides` : ''}${c.keys && c.keys.length ? `. Paths in it: ${c.keys.map(([k, v]) => `${k} = ${code(v)}`).join(', ')}` : ''}`);
  else out.push('- No config file found (config/, settings.*, appsettings.*, config.*)');
  if (c.others && c.others.length) out.push(`- Other config files: ${c.others.slice(0, 5).map((o) => `${code(o.path)} (read by ${o.readBy})`).join(', ')}`);
  if (c.secrets.length) out.push(`- Secrets: ${c.secrets.map(code).join(', ')} (names only; never read, print or commit)`);
  if (F.env.template || F.env.others.length) out.push(`- Env template for a new install: ${F.env.template ? code(F.env.template) : 'none current'}${F.env.others.length ? `; not current: ${F.env.others.map(code).join(', ')} (name marks an old copy)` : ''}`);
  if (F.readme.current || F.readme.others.length) out.push(`- Current README: ${F.readme.current ? code(F.readme.current) : 'none at the top level'}${F.readme.others.length ? `; also at the top level: ${F.readme.others.map(code).join(', ')} (owner decides; not current unless the owner says so)` : ''}`);
  return out;
}
function outputLines(F) {
  const out = F.outputs.map((o) => `- ${code(`${o.path}/`)} — ${o.evidence.join('; ')}`);
  out.push(...F.external.map((e) => `- ${code(e.path)} (outside the repo) — written by ${e.by.map(code).join(', ')}`));
  return out.length ? out : ['- none found'];
}

// ---------------------------------------------------------------- the card: AGENTS.md managed block, generated from evidence
const CARD_MAX = 150;
function currentLine(text) { const m = /^\W*Current\b[^:\n]{0,60}:\**\s*(.+)$/m.exec(text || ''); return m ? m[1].trim() : ''; }
function sectionLines(text, heading, max) {
  const lines = (text || '').split(/\r?\n/); const i = lines.findIndex((l) => new RegExp(`^#+\\s*${heading}\\b`, 'i').test(l));
  if (i < 0) return [];
  const out = [];
  for (const l of lines.slice(i + 1)) { if (/^#+\s/.test(l)) break; if (!l.trim() || /^\s*<!--/.test(l)) continue; out.push(l); }
  return out.slice(0, max);
}
/** Render the card. Returns {text, overflow[]} — overflow lines go to the INDEX so nothing is lost. */
export function renderCard(ctx, F, state) {
  const L = ctx.cfg.layout; const S = '.filing/filing.mjs';
  const cur = readText(absOf(ctx.root, L.current)) || '';
  const head = [`${BLOCK_BEGIN} v2 · generated by "node ${S} render" from the files it names; edit outside this block -->`,
    '## Project card (generated from evidence: read this first)', '',
    `Every answer is here or in ONE file named here. All docs, one line each, open items and the last session: ${code(L.index)}.`, ''];
  const body = [];
  body.push('### Status', '', `- **Current** (owner, ${code(L.current)}, ${declaredDate(cur) || 'UNDATED'}): ${delink(currentLine(cur), L.current, ctx) || 'NOT NAMED: the owner must name it'}`);
  const prog = sectionLines(cur, 'In progress', 12).map((l) => delink(l, L.current, ctx));
  // Ledger items and the last session's next command live in the INDEX (merge=union), not here: the card changes only
  // when evidence or STATUS changes, so parallel branches adding items never conflict on AGENTS.md.
  if (prog.length) body.push(`- **In progress** (from ${code(L.current)}):`, ...prog.map((l) => `  ${l.replace(/^\s*/, '')}`));
  body.push(`- **${prog.length ? '' : 'In progress, '}Open items, the exact next command, and what the last session left UNVERIFIED:** ${code(L.index)}`);
  body.push('', '### Config, README, env', '', ...configLines(F));
  body.push('', `### Schedules (read from ${F.sources.length ? F.sources.map(code).join(', ') : 'nothing: no crontab, task XML or timer in the repo'})`, '');
  if (F.schedules.length) body.push(...F.schedules.flatMap(schedLines)); else body.push('- none found in the repo');
  body.push(...F.retired.map(retiredLine));
  body.push('', '### Where outputs go', '', ...outputLines(F));
  body.push('', '### Runbooks (open the one you need)', '', ...(F.runbooks.length ? F.runbooks.slice(0, 15).map((r) => `- ${code(r.path)} — ${r.purpose}`) : ['- none found (no doc named runbook, restore, recover, rotate, troubleshoot, incident…)']));
  if (F.runbooks.length > 15) body.push(`- +${F.runbooks.length - 15} more in ${code(L.index)}`);
  body.push('', '### Superseded docs', '');
  if (F.archive.length) { for (const a of F.archive.slice(-8)) body.push(`- ${code(a.from)} → ${code(a.to)}, superseded by ${a.by}`); if (F.archive.length > 8) body.push(`- +${F.archive.length - 8} more in ${code(`${L.archive}/INDEX.tsv`)}`); }
  else if (!F.archiveUnindexed.length) body.push(`- none archived (${code(`${L.archive}/INDEX.tsv`)})`);
  if (F.archiveUnindexed.length) body.push(`- ${F.archiveUnindexed.length} older cop${F.archiveUnindexed.length === 1 ? 'y' : 'ies'} moved into ${code(`${L.archive}/`)} by a reorganisation (same relative path inside it; the survey plan says why)`);
  // Constant text, not the list: two branches that each add a correction then produce the same card (no conflict on AGENTS.md).
  if ((state.lessons || []).some((l) => !l.retired)) body.push('', '### Standing corrections', '', `- The owner has corrected past sessions. Read "Standing corrections" in ${code(L.index)} BEFORE you start, and do not repeat them.`);
  body.push('', '### Rules', '',
    `- Record work with \`node ${S} item add|start|done|note\`; never hand-edit this block, ${code(L.index)} or the ledger. After any change: \`node ${S} render\`.`,
    '- Never move or rename a file that code, cron or a scheduled task names (listed above). Moves go through `survey` → owner → `apply` (undo-able).',
    `- Never rewrite an existing doc to "fix" it during a reorganisation: archive it unchanged (\`node ${S} archive <old> --by <what replaces it>\`) and write new docs as new files.`,
    `- Done means \`node ${S} check\` exits 0; end with \`node ${S} close "<summary>" --next "<exact command>"\`.`);
  const end = ['<!-- filing:end -->'];
  let lines = [...head, ...body]; let overflow = [];
  if (lines.length + end.length > CARD_MAX) {
    const keep = CARD_MAX - end.length - 2;
    overflow = lines.slice(keep); lines = [...lines.slice(0, keep), '', `(card full: ${overflow.length} more lines in ${code(L.index)}, section "Card overflow")`];
  }
  return { text: [...lines, ...end].join('\n'), overflow };
}
function cardBlockOf(text) {
  if (text == null) return null;
  const b = text.indexOf(BLOCK_BEGIN); const e = text.indexOf(BLOCK_END);
  return b >= 0 && e > b ? lf(text.slice(b, e + BLOCK_END.length)) : null;
}

// ---------------------------------------------------------------- cold start: entry file + at most one hop
export const STANDARD_QUESTIONS = [
  ['current', 'What is current?'], ['in-progress', 'What is in progress, and what is next?'], ['config', 'Which config file is the source of truth?'],
  ['schedules', 'What runs on a schedule, and which steps does each run?'], ['outputs', 'Where do outputs go?'], ['backups', 'Where do backups go?'],
  ['runbooks', 'Which runbook covers a failure (restore, rotate, recover)?'], ['readme', 'Which README is current?'], ['env', 'Which env example does a new install copy?'],
  ['superseded', 'Which docs are superseded, and by what?'],
];
/** An "In progress" / "Next command" label followed by content, not just a pointer to a file. */
function inProgressAnswered(ctx, text) {
  const lines = text.split('\n');
  // backticked spans are pointers (a file, or a command to go and run) unless the label asks for a command
  const pointerOnly = (s, codeCounts = false) => !s.replace(/`([^`]+)`/g, (w, p2) => (codeCounts && !ctx.fileSet.has(p2.replace(/^\.\//, '')) ? 'x' : '')).replace(/\b(written|and|dated|by|the|owner|generated|do not edit|see|in|at)\b/gi, '').replace(/[\s.,;:()*_-]/g, '').trim();
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    if (/^#+\s/.test(l) ? !/^#+\s*(in[ -]progress|open items|doing|next( command| steps)?)\b/i.test(l) : !/(in[ -]progress|next command|open items|^\W*doing\b)/i.test(l)) continue;
    if (/^#+\s/.test(l) || /:\**\s*$/.test(l)) { const nxt = lines.slice(i + 1).find((x) => x.trim() && !/^\s*<!--/.test(x)); if (nxt && !/^#+\s/.test(nxt) && !pointerOnly(nxt.replace(/^\s*[-*\d.]+\s*/, ''))) return true; continue; }
    const k = l.indexOf(':'); if (k < 0) continue;
    if (!pointerOnly(l.slice(k + 1), /next command/i.test(l.slice(0, k)))) return true;
  }
  return false;
}
/** Repo files and folders a text names (backticks, links, bare paths). */
function mentioned(ctx, text) {
  const out = new Set();
  for (const m of text.matchAll(/`([^`\n]+)`|\]\(<?([^)>\s]+)>?\)/g)) {
    let t = (m[1] || m[2] || '').trim().replace(/\\/g, '/').replace(/^\.\//, '').replace(/[#?].*$/, ''); try { t = decodeURIComponent(t); } catch { /* raw */ }
    for (const c of [t, t.replace(/\/$/, ''), ...t.split(/\s+/)]) if (ctx.fileSet.has(c)) out.add(c);
  }
  return [...out];
}
const mentions = (text, p2) => text.includes(p2) || text.includes(p2.replace(/ /g, '%20'));
function entryText(ctx) {
  for (const f of ['AGENTS.md', 'CLAUDE.md', 'GEMINI.md']) {
    const t = readText(path.join(ctx.root, f)); if (t == null) continue;
    // imports are loaded automatically with the entry file, so they cost no hop
    const expanded = t.replace(/^@(\.\/)?(\S+)\s*$/gm, (w, a, rel) => (rel !== f && ctx.fileSet.has(rel) ? readText(absOf(ctx.root, rel)) || w : w));
    return { file: f, text: lf(expanded) };
  }
  return { file: null, text: '' };
}
/**
 * Simulate a cold-start agent: it reads the auto-loaded entry file, then may open ONE file the entry names.
 * For each standard question, report where the answer is ('entry', 'hop: <file>'), 'n/a' (the repo has nothing
 * of that kind), or 'missing' with what could not be found.
 */
export function coldStart(ctx, F) {
  const entry = entryText(ctx);
  const hops = mentioned(ctx, entry.text).filter((f) => /\.(md|markdown|txt|json|ya?ml|toml|ini|cfg|xml|tsv|csv|sh|ps1|py)$/i.test(f) || !P.extname(f))
    .map((f) => ({ f, text: lf(decodeText((() => { try { return fs.readFileSync(absOf(ctx.root, f)); } catch { return Buffer.alloc(0); } })())) }));
  const need = (list) => (text) => { const miss = list.filter((p2) => !mentions(text, p2)); return miss.length ? `not named: ${miss.slice(0, 4).join(', ')}${miss.length > 4 ? ` (+${miss.length - 4})` : ''}` : null; };
  const sched = F.schedules.filter((s) => s.target || s.raw);
  const backups = sched.filter((s) => /backup/i.test([s.target, ...(s.steps || [])].join(' ')));
  const Q = {
    current: (t) => { const v = currentLine(t); return v && !/NOT NAMED|\(owner: name/.test(v) ? null : 'no "Current:" line naming the current thing'; },
    'in-progress': (t) => (inProgressAnswered(ctx, t) ? null : 'no "In progress" section with content (a pointer to another file is not an answer)'),
    config: F.config.source ? need([F.config.source]) : null,
    schedules: sched.length ? need(sched.flatMap((s) => [s.target || s.raw, ...(s.steps || [])])) : null,
    outputs: F.outputs.length || F.external.length ? need([...F.outputs.map((o) => `${o.path}/`), ...F.external.map((e) => e.path)]) : null,
    backups: backups.length ? need(backups.flatMap((s) => (s.writes.length ? s.writes : [s.target || s.raw]))) : null,
    runbooks: F.runbooks.length ? 'entry-only' : null,
    readme: F.readme.current ? need([F.readme.current]) : null,
    env: F.env.template ? need([F.env.template]) : null,
    superseded: F.archive.length || F.archiveUnindexed.length ? (t) => (mentions(t, ctx.cfg.layout.archive) || (F.archive.length && F.archive.every((a) => mentions(t, a.from))) ? null : `archive ${ctx.cfg.layout.archive}/ not named`) : null,
  };
  return STANDARD_QUESTIONS.map(([id, question]) => {
    const fn = Q[id];
    if (!fn) return { id, question, status: 'n/a' };
    if (!entry.file) return { id, question, status: 'missing', why: 'no AGENTS.md, CLAUDE.md or GEMINI.md' };
    if (fn === 'entry-only') { const miss = F.runbooks.filter((r) => !mentions(entry.text, r.path)).map((r) => r.path); return miss.length ? { id, question, status: 'missing', why: `runbooks not named in ${entry.file}: ${miss.slice(0, 4).join(', ')}` } : { id, question, status: 'entry' }; }
    const why = fn(entry.text); if (!why) return { id, question, status: 'entry' };
    for (const h of hops) if (!fn(`${entry.text}\n${h.text}`)) return { id, question, status: `hop: ${h.f}` };
    return { id, question, status: 'missing', why };
  });
}

// ---------------------------------------------------------------- rendering (deterministic: no wall clock, relative paths)
const GEN_NOTE = (src) => `<!-- generated by .filing/filing.mjs from ${src} — do not edit; run: node .filing/filing.mjs render -->`;
function linkFrom(fromRel, toRel) { const r = P.relative(P.dirname(fromRel), toRel) || P.basename(toRel); return /[\s()<>]/.test(r) ? `<${r}>` : r; }

/** INDEX.md: every doc in one line, open items, the last session, and anything the card had no room for. */
export function renderIndex(ctx, docs, state, overflow = []) {
  const I = ctx.cfg.layout.index;
  const items = [...state.items.values()]; const last = state.closes[state.closes.length - 1];
  const pkgs = [...new Set(items.map((i) => i.pkg))].sort();
  const line = (i) => `- \`${i.id}\` ${i.title}${i.home ? ` — home: [${i.home}](${linkFrom(I, i.home)})` : ''}${i.owner ? ` — owner: ${i.owner}` : ''} — ${i.status} since ${i.since.slice(0, 10)} (${i.by})`;
  const group = (sts) => { const out = []; for (const p of pkgs) { const list = items.filter((i) => sts.includes(i.status) && i.pkg === p); if (!list.length) continue; if (pkgs.length > 1) out.push(`**${p || '(root)'}**`); out.push(...list.map(line)); } return out.length ? out : ['- none']; };
  const rows = docs.map((d) => { const t = readText(absOf(ctx.root, d)) || ''; return `| [${d}](${linkFrom(I, d)}) | ${summaryOf(t)} | ${declaredDate(t) || '—'} |`; });
  const L = [GEN_NOTE('the docs below and .filing/ledger.jsonl'), '# Index', '',
    'Dates are UTC. The card at the top of `AGENTS.md` holds the facts; this file holds the long lists. Live git state: `node .filing/filing.mjs status`.', '',
    ...(() => { const a = (state.lessons || []).filter((l) => !l.retired); return a.length ? ['## Standing corrections', '', ...a.map((l) => `- \`${l.id}\` ${l.wrong} → ${l.instead} (${l.ts.slice(0, 10)}, ${l.by})`), ''] : []; })(),
    '## Open items', '', ...group(['doing', 'open']), '',
    '## Last session', '',
    ...(last ? [`- ${last.ts.slice(0, 10)} by ${last.who}: ${last.summary}`, `- Next command: \`${last.next}\``,
      `- git at close: ${last.branch || '(no git)'} @ ${last.sha || '-'}${last.dirty ? `, ${last.dirty} uncommitted file(s)` : ''}; fetch: ${last.fetch || 'n/a'}`,
      ...(last.verified || []).map((v) => `- Verified: ${v}`), ...(last.unverified || []).map((v) => `- **UNVERIFIED:** ${v}`), ...(last.blockers || []).map((v) => `- Blocker: ${v}`),
      ...(last.session ? [`- log: [${last.session}](${linkFrom(I, last.session)})`] : [])] : ['- no session closed yet']), '',
    '## Recently done', '', ...(() => { const d = items.filter((i) => i.status === 'done').sort((a, b) => (a.since < b.since ? 1 : -1)).slice(0, 10); return d.length ? d.map((i) => `- \`${i.id}\` ${i.title} (${i.since.slice(0, 10)})`) : ['- none']; })(), '',
    '## Docs', '', 'One line per doc. "Last true" is the date the doc declares (`Last updated: YYYY-MM-DD` or frontmatter `updated:`).', '',
    '| Doc | Summary | Last true |', '|---|---|---|', ...rows, ''];
  if (overflow.length) L.push('## Card overflow', '', ...overflow, '');
  return L.join('\n');
}
function manifest(ctx) {
  const rows = ['path\tsize\tsha256'];
  for (const f of ctx.files) { if (f.startsWith('.filing/')) continue; const a = absOf(ctx.root, f); let st; try { st = fs.statSync(a); } catch { continue; } rows.push(`${f}\t${st.size}\t${st.size <= 1 << 20 ? sha(fs.readFileSync(a)) : 'size-only'}`); }
  return `${rows.join('\n')}\n`;
}
/** The card and the index, as they should be now. */
export function rendered(ctx, state = fold(readLedger(ctx.root))) {
  ctx.dirSet = ctx.dirSet || dirSetOf(ctx.files);
  const F = facts(ctx); const card = renderCard(ctx, F, state);
  return { F, card: card.text, index: renderIndex(ctx, docSet(ctx), state, card.overflow) };
}
function renderAll(ctx) {
  const r = rendered(ctx); const changed = [];
  const L = ctx.cfg.layout;
  const ag = readText(path.join(ctx.root, 'AGENTS.md'));
  if (ag != null && ag.includes(BLOCK_BEGIN) && writeIfChanged(path.join(ctx.root, 'AGENTS.md'), upsertBlock(lf(ag), r.card))) changed.push('AGENTS.md');
  if (writeIfChanged(absOf(ctx.root, L.index), r.index)) changed.push(L.index);
  if (!ctx.git) { if (writeIfChanged(path.join(ctx.root, '.filing', 'manifest.tsv'), manifest(ctx))) changed.push('.filing/manifest.tsv'); }
  return changed;
}

// ---------------------------------------------------------------- check
export function check(root, opts = {}) {
  const ctx = context(root); const L = ctx.cfg.layout;
  const fail = []; const warn = []; const info = [];
  if (!exists(path.join(root, '.filing', 'config.json'))) { fail.push({ code: 'NOT-INITIALISED', msg: 'no .filing/config.json; run init from the skill' }); return { fail, warn, info }; }
  const cur = readText(absOf(root, L.current));
  if (cur == null) fail.push({ code: 'CURRENT', path: L.current, msg: 'missing: the one place that says what is current' });
  else {
    if (!declaredDate(cur)) fail.push({ code: 'CURRENT', path: L.current, msg: 'undated: add "Last updated: YYYY-MM-DD"' });
    if (/\(owner: name the one current thing\)/.test(cur)) fail.push({ code: 'CURRENT', path: L.current, msg: 'names nothing yet: the owner must name the one current thing' });
  }
  const state = fold(readLedger(root));
  state.errors.forEach((e) => fail.push({ code: 'LEDGER', msg: e }));
  const pkgOk = (i) => !opts.pkg || i.pkg === opts.pkg;
  const doing = [];
  for (const i of state.items.values()) {
    if (!pkgOk(i) || !['open', 'doing'].includes(i.status)) continue;
    if (i.home && !exists(absOf(root, i.home))) fail.push({ code: 'BAD-HOME', msg: `item ${i.id} lives in "${i.home}", which does not exist` });
    if (i.status === 'doing') doing.push(i);
  }
  for (let a = 0; a < doing.length; a++) for (let b = a + 1; b < doing.length; b++) {
    const o = (doing[a].files || []).filter((f) => (doing[b].files || []).includes(f));
    if (o.length) warn.push({ code: 'CLAIM-OVERLAP', msg: `items ${doing[a].id} and ${doing[b].id} both claim ${o.join(', ')}` });
  }
  // generated: the card in AGENTS.md and INDEX.md must equal a fresh render (compared in memory; CRLF-insensitive)
  const docs = docSet(ctx);
  const R = rendered(ctx, state);
  const agentsText = readText(path.join(root, 'AGENTS.md'));
  const block = cardBlockOf(agentsText);
  if (agentsText != null && block != null && block !== R.card) fail.push({ code: 'GENERATED', path: 'AGENTS.md', msg: /filing:begin v1/.test(block) ? 'the card is the old v1 signpost: run render to replace it with the facts card' : 'the card differs from the evidence (a schedule, config, status or item changed, or it was hand-edited): run render' });
  const idx = lf(readText(absOf(root, L.index)));
  if (idx !== R.index) {
    const listed = new Set([...(idx || '').matchAll(/^\| \[([^\]]+)\]/gm)].map((m) => m[1]));
    const newDocs = docs.filter((d) => !listed.has(d));
    fail.push({ code: 'INDEX', path: L.index, msg: idx == null ? 'missing: run render' : newDocs.length ? `not indexed: ${newDocs.join(', ')} (run render)` : 'out of date (an item, a session or a doc changed): run render' });
  }
  if (L.handoff && exists(absOf(root, L.handoff))) warn.push({ code: 'LEGACY', path: L.handoff, msg: 'no longer generated: its content is now in the AGENTS.md card and the INDEX. Archive it: node .filing/filing.mjs archive ' + L.handoff + ' --by AGENTS.md' });
  // archived files are kept byte for byte: the hash recorded at archive time must still match
  const archived = R.F.archive;
  for (const a of archived) {
    if (!a.hash || !a.to) continue;
    if (!exists(absOf(root, a.to))) fail.push({ code: 'ARCHIVE', path: a.to, msg: 'archived file is missing (the archive never loses files)' });
    else if (hashFile(absOf(root, a.to)) !== a.hash) fail.push({ code: 'ARCHIVE', path: a.to, msg: 'archived file was edited: superseded docs stay byte for byte; put new content in a new file' });
    if (ctx.fileSet.has(a.from) && a.from !== L.index) warn.push({ code: 'REUSED-PATH', path: a.from, msg: `was archived on ${a.date} and a different file now sits at the same path: give a new doc a new name so links and history stay unambiguous` });
  }
  // cold start: the entry file plus at most one file it names must answer every standard question
  const cold = coldStart(ctx, R.F);
  for (const q of cold) if (q.status === 'missing') warn.push({ code: 'COLD-START', msg: `"${q.question}" is not answerable from AGENTS.md plus one file it names (${q.why})` });
  if (opts.cold) for (const q of cold) info.push({ code: 'COLD-START', msg: `${q.status.padEnd(28)} ${q.question}` });
  // stale / broken
  const scope = opts.pkg ? docs.filter((d) => d.startsWith(`${opts.pkg}/`)) : docs;
  const an = analyzeDocs(ctx, [...new Set([...scope, L.current].filter((d) => ctx.fileSet.has(d)))]);
  fail.push(...an.fail); warn.push(...an.warn);
  // prose says done but the ledger does not
  for (const i of state.items.values()) {
    if (!['open', 'doing'].includes(i.status) || i.id.length < 3) continue;
    const re = new RegExp(`\\b${i.id.replace(/[-.]/g, '\\$&')}\\b.*\\b(done|complete[d]?|finished|shipped|closed)\\b`, 'i');
    for (const [d, t] of an.texts) if (re.test(t)) warn.push({ code: 'PROSE-DONE', path: d, msg: `says item ${i.id} is done, but the ledger has it ${i.status}: run "item done ${i.id}" or fix the doc` });
  }
  // entry points for every agent tool
  const agents = readText(path.join(root, 'AGENTS.md'));
  if (agents == null) fail.push({ code: 'AGENTS', msg: 'AGENTS.md missing: no agent tool will find the project state' });
  else {
    const b = agents.indexOf(BLOCK_BEGIN); const e = agents.indexOf(BLOCK_END);
    if (b < 0 || e < b) fail.push({ code: 'AGENTS', msg: 'AGENTS.md has no filing block: re-run init' });
    else if (agents.slice(b, e).split('\n').length > CARD_MAX) fail.push({ code: 'AGENTS', msg: `card over ${CARD_MAX} lines: run render (it moves the excess to the INDEX)` });
    if (Buffer.byteLength(agents) > 32768) fail.push({ code: 'AGENTS', msg: 'AGENTS.md over 32 KiB: Codex truncates project docs at that size by default; move detail to docs/' });
    else if (agents.split('\n').length > 220) warn.push({ code: 'AGENTS', msg: 'AGENTS.md over 220 lines: it is loaded on every turn; keep your own text outside the card short' });
  }
  if (exists(path.join(root, 'AGENTS.override.md'))) warn.push({ code: 'AGENTS', msg: 'AGENTS.override.md exists: Codex reads it INSTEAD of AGENTS.md in this folder' });
  if (exists(path.join(root, 'knowledge.md'))) warn.push({ code: 'AGENTS', msg: 'knowledge.md found: current Codebuff/Freebuff reads AGENTS.md first; fold it into AGENTS.md' });
  const claude = readText(path.join(root, 'CLAUDE.md'));
  if (claude != null && !/^@\.?\/?AGENTS\.md\s*$/m.test(claude)) warn.push({ code: 'AGENTS', msg: 'CLAUDE.md does not import AGENTS.md (add a line "@AGENTS.md")' });
  if (!geminiCovered(root)) warn.push({ code: 'AGENTS', msg: 'Gemini CLI will not see AGENTS.md: add GEMINI.md with "@./AGENTS.md", or list AGENTS.md in .gemini/settings.json context.fileName' });
  for (const p of ctx.cfg.packages || []) if (!(readText(path.join(absOf(root, p), 'AGENTS.md')) || '').includes(BLOCK_BEGIN)) warn.push({ code: 'AGENTS', path: p, msg: 'package has no AGENTS.md filing block' });
  if (ctx.git && !/ledger\.jsonl[^\n]*merge=union/.test(readText(path.join(root, '.gitattributes')) || '')) warn.push({ code: 'GIT', msg: '.gitattributes lacks ".filing/ledger.jsonl merge=union": parallel branches will conflict' });
  // lease
  let lease = null; try { lease = JSON.parse(readText(path.join(root, '.filing', 'lease.json'))); } catch { /* none */ }
  if (lease && !lease.released && leaseDead(lease)) info.push({ code: 'LEASE', msg: `expired lease from ${lease.holder} (${lease.cmd}); the next writer takes it over` });
  if (!ctx.git) {
    const old = new Map((readText(path.join(root, '.filing', 'manifest.tsv')) || '').split('\n').slice(1).filter(Boolean).map((l) => { const [p, s, h2] = l.split('\t'); return [p, `${s}\t${h2}`]; }));
    if (old.size) {
      const now = new Map(manifest(ctx).split('\n').slice(1).filter(Boolean).map((l) => { const [p, s, h2] = l.split('\t'); return [p, `${s}\t${h2}`]; }));
      for (const [p, v] of now) if (!old.has(p)) info.push({ code: 'NEW', path: p, msg: 'since last render' }); else if (old.get(p) !== v) info.push({ code: 'CHANGED', path: p, msg: 'since last render' });
      for (const p of old.keys()) if (!now.has(p)) info.push({ code: 'MISSING', path: p, msg: 'since last render' });
    }
  }
  return { fail, warn, info };
}
function geminiCovered(root) {
  const g = readText(path.join(root, 'GEMINI.md'));
  if (g != null && /^@\.?\/?AGENTS\.md\s*$/m.test(g)) return true;
  try { const s = JSON.parse(readText(path.join(root, '.gemini', 'settings.json'))); const fn = s && s.context && s.context.fileName; return [].concat(fn || []).includes('AGENTS.md'); } catch { return false; }
}

// ---------------------------------------------------------------- link rewriting (used by apply and archive)
/**
 * Rewrite relative markdown links in `content` (a doc moving oldRel → newRel) given a move map
 * old→new for other files. Code spans/blocks are untouched. Returns the new content.
 */
export function rewriteLinks(content, oldRel, newRel, moves, existedBefore) {
  let out = '';
  for (const seg of mdSegments(content)) {
    if (seg.code) { out += seg.s; continue; }
    out += seg.s.replace(LINK_RE, (whole, a1, t1, a3, d1, t2) => {
      const raw = t1 || t2; if (isExternal(raw.replace(/^</, ''))) return whole;
      const t = splitTarget(raw);
      const oldTarget = P.normalize(P.join(P.dirname(oldRel), t.decoded));
      if (oldTarget.startsWith('../') || !existedBefore(oldTarget)) return whole;
      const newTarget = moves.get(oldTarget) || oldTarget;
      if (newTarget === oldTarget && newRel === oldRel) return whole;
      let rel = P.relative(P.dirname(newRel), newTarget) || P.basename(newTarget);
      if (t.decoded.endsWith('/') && !rel.endsWith('/')) rel += '/';
      if (rel === P.normalize(t.decoded) || (t.decoded.startsWith('./') && rel === P.normalize(t.decoded.slice(2)))) return whole;
      if (t.decoded.startsWith('./') && !rel.startsWith('../')) rel = `./${rel}`;
      let written;
      if (t.angle) written = `<${rel}${t.suffix}>`;
      else if (t.encoded || /\s/.test(rel)) written = t.encoded ? encodeURI(rel) + t.suffix : `<${rel}${t.suffix}>`;
      else written = rel + t.suffix;
      return t1 ? `${a1}${written}${a3}` : `${d1}${written}`;
    });
  }
  return out;
}

// ---------------------------------------------------------------- survey (read-only)
export function familyKey(base) {
  const ext = P.extname(base).toLowerCase();
  let s = base.slice(0, base.length - ext.length).toLowerCase().replace(/[_\-.]+/g, ' ');
  const marker = /\(\d+\)|\bcopy( of)?\b|\b(final|real|latest|new|old|backup|bak|draft|wip|updated|revised|edited|fixed)\b|\bv\d+(\s\d+)*\b|\b\d{8}\b|\b\d{4}\s\d{2}\s\d{2}\b/g;
  const had = marker.test(s); marker.lastIndex = 0;
  s = s.replace(marker, ' ').replace(/\s+/g, ' ').trim();
  return { key: s ? `${s}${ext === '.markdown' ? '.md' : ext}` : '', marked: had };
}
const ROLE = /(current|handoff|status|todo|roadmap|plan|next[ _-]?steps)/i;

function hashFile(abs) { return sha(fs.readFileSync(abs)); }

export function survey(root, opts = {}) {
  const ctx = context(root); ctx.dirSet = dirSetOf(ctx.files);
  const L = ctx.cfg.layout; const res = { version: 1, tool: `filing.mjs ${VERSION}`, git: ctx.git, files: ctx.files.length, layout: L,
    patterns: {}, entries: [], decisions: [], duplicates: [], pinned: [], secrets: [], transfers: [], builds: [], hazards: [],
    brokenLinks: [], stale: [], crlfDocs: 0, ignoredPresent: [], cloudOnly: [], limits: [] };
  const stat = new Map();
  for (const f of ctx.files) { try { stat.set(f, fs.statSync(absOf(root, f))); } catch { /* vanished */ } }
  const secret = (f) => SECRET_NAME.test(P.basename(f));
  // secrets: names only, contents never read
  res.patterns.secrets = SECRET_NAME.source;
  for (const f of ctx.files) if (secret(f)) res.secrets.push({ path: f, tracked: ctx.git && git(root, ['ls-files', '--error-unmatch', '--', `:(literal)${f}`]).ok, note: 'not gitignored — contents not read' });
  // build outputs and transfers
  res.patterns.builds = [...BUILD_DIRS].join(',');
  const builds = new Set(); for (const f of ctx.files) { const seg = f.split('/'); const k = seg.findIndex((s) => BUILD_DIRS.has(s)); if (k >= 0 && k < seg.length - 1) builds.add(seg.slice(0, k + 1).join('/')); }
  res.builds = [...builds].sort();
  const inBuild = (f) => res.builds.some((b) => f.startsWith(`${b}/`));
  res.patterns.transfers = TRANSFER.source;
  res.transfers = ctx.files.filter((f) => TRANSFER.test(f) && !inBuild(f)).map((f) => ({ path: f, size: stat.get(f)?.size, note: 'transfer/backup copy: follow references/carry-forward.md before any deletion' }));
  if (ctx.git) res.ignoredPresent = git(root, ['ls-files', '-oi', '--exclude-standard', '--directory', '-z']).out.split('\0').filter((x) => x && !INTERNAL.test(x)).slice(0, 50);
  // Windows hazards
  const lower = new Map();
  for (const f of [...ctx.files, ...ctx.dirSet]) { const k = f.toLowerCase(); if (lower.has(k) && lower.get(k) !== f) res.hazards.push({ path: f, problem: `differs only by case from "${lower.get(k)}"` }); else lower.set(k, f); }
  for (const f of ctx.files) {
    for (const seg of f.split('/')) {
      if (RESERVED.test(seg)) res.hazards.push({ path: f, problem: `"${seg}" is a reserved name on Windows` });
      if (/[. ]$/.test(seg)) res.hazards.push({ path: f, problem: `"${seg}" ends with a dot or space (Windows strips it)` });
      if (/[<>:"|?*]/.test(seg)) res.hazards.push({ path: f, problem: `"${seg}" has a character Windows forbids` });
    }
    if (path.resolve(root).length + 1 + f.length >= 240) res.hazards.push({ path: f, problem: `path is ${path.resolve(root).length + 1 + f.length} characters here (Windows tools break near 260)` });
    const st = stat.get(f);
    if (st && st.size > 0 && st.blocks === 0 && process.platform !== 'win32') res.cloudOnly.push(f); // NTFS reports blocks=0 for small MFT-resident files, so this heuristic is POSIX-only
  }
  if (process.platform === 'win32') res.limits.push('cloud-only placeholder detection is unavailable on Windows; only same-size files are read for duplicate hashing');
  // pinned: any file named by code, config, scheduled-task or script files stays put
  const codeFiles = ctx.files.filter((f) => (CODE_EXT.has(P.extname(f).toLowerCase()) || CODE_NAMES.test(P.basename(f))) && !secret(f) && !inBuild(f) && (stat.get(f)?.size || 0) <= 2 << 20);
  res.patterns.pinnedScan = `${codeFiles.length} code/config/scheduler files scanned (${[...CODE_EXT].join(' ')}, Dockerfile, Makefile, crontab, *.service, *.timer, *.plist); secret-named files were not read`;
  const byBase = new Map(); for (const f of ctx.files) { const b = P.basename(f).toLowerCase(); if (!byBase.has(b)) byBase.set(b, []); byBase.get(b).push(f); }
  const spaced = [...byBase.keys()].filter((b) => /\s/.test(b));
  const pinBy = new Map(); const pin = (f, by) => { if (f === by) return; if (!pinBy.has(f)) pinBy.set(f, new Set()); pinBy.get(f).add(by); };
  for (const cf of codeFiles) {
    const text = readText(absOf(root, cf)); if (!text) continue;
    const low = text.toLowerCase();
    for (const raw of low.split(/[\s"'`<>|;,(){}[\]=]+/)) {
      if (!raw || raw.length < 3) continue;
      const tok = raw.replace(/\\/g, '/').replace(/^[a-z]:\//, '/').replace(/[:.]+$/, '');
      const segs = tok.split('/').filter(Boolean); if (!segs.length) continue;
      const cands = byBase.get(segs[segs.length - 1]);
      if (cands) {
        const narrowed = segs.length > 1 ? cands.filter((c) => (`/${c.toLowerCase()}`).endsWith(`/${segs.slice(-2).join('/')}`)) : [];
        for (const c of narrowed.length ? narrowed : cands) pin(c, cf);
      }
      if (tok.endsWith('/') && segs.length) for (const d of ctx.dirSet) if ((`/${d.toLowerCase()}`).endsWith(`/${segs.join('/')}`)) for (const f of ctx.files) if (f.startsWith(`${d}/`)) pin(f, cf);
    }
    for (const b of spaced) if (low.includes(b)) for (const c of byBase.get(b)) pin(c, cf);
  }
  for (const f of ctx.files) if (anyGlob(ctx.cfg.pinned || [], f)) pin(f, '.filing/config.json pinned');
  res.pinned = [...pinBy].map(([f, by]) => ({ path: f, by: [...by].slice(0, 3) })).sort((a, b) => (a.path < b.path ? -1 : 1));
  const pinned = new Set(pinBy.keys());
  // dirty tracked files must not move (commit first) — keeps undo exact
  const dirty = new Set(); if (ctx.git) for (const l of git(root, ['status', '--porcelain', '-z', '--untracked-files=no']).out.split('\0')) if (l.length > 3) dirty.add(l.slice(3));
  const tracked = ctx.git ? new Set(git(root, ['ls-files', '-z']).out.split('\0').filter(Boolean)) : new Set();
  // duplicates: same size, then same hash
  const bySize = new Map(); for (const f of ctx.files) { const s = stat.get(f); if (!s || !s.size || res.cloudOnly.includes(f) || secret(f) || f.startsWith('.filing/')) continue; if (!bySize.has(s.size)) bySize.set(s.size, []); bySize.get(s.size).push(f); }
  const hashes = new Map();
  for (const group of bySize.values()) if (group.length > 1) for (const f of group) hashes.set(f, hashFile(absOf(root, f)));
  const byHash = new Map(); for (const [f, h] of hashes) { if (!byHash.has(h)) byHash.set(h, []); byHash.get(h).push(f); }
  const linked = new Set(); const mdDocs = ctx.files.filter((f) => /\.(md|markdown)$/i.test(f) && !inBuild(f));
  for (const d of mdDocs) { const t = readText(absOf(root, d)); if (!t) continue; if (/\r\n/.test(t)) res.crlfDocs++; for (const l of mdLinks(t)) { const r = resolveRef(ctx, d, l); if (r) linked.add(r); } }
  const archiveOf = (f) => `${L.archive}/${f}`;
  const moves = new Map(); const redirect = new Map(); const reasons = new Map();
  const movable = (f) => !pinned.has(f) && !secret(f) && !inBuild(f) && !dirty.has(f) && !f.startsWith('.filing/') && !f.startsWith(`${L.archive}/`) && !TRANSFER.test(f);
  for (const group of byHash.values()) {
    if (group.length < 2) continue;
    const keep = [...group].sort((a, b) => (pinned.has(b) - pinned.has(a)) || (linked.has(b) - linked.has(a)) || (a.split('/').length - b.split('/').length) || (a.length - b.length) || (a < b ? -1 : 1))[0];
    res.duplicates.push({ keep, copies: group.filter((g) => g !== keep) });
    for (const g of group) if (g !== keep && movable(g) && DOC_EXT.has(P.extname(g).toLowerCase()) && !ROOT_KEEP.has(P.basename(g).toLowerCase())) { moves.set(g, archiveOf(g)); redirect.set(g, keep); reasons.set(g, [`exact duplicate of ${keep}`, 'high']); }
  }
  // version families and competing "current" docs: the owner decides
  const fam = new Map();
  // grouped per folder: a README or NOTES in each folder is that folder's own doc, not a version of another folder's
  for (const f of ctx.files) { if (inBuild(f) || f.startsWith('.filing/') || f.startsWith(`${L.archive}/`)) continue; const fk = familyKey(P.basename(f)); if (!fk.key) continue; const key = `${P.dirname(f)}/${fk.key}`; if (!fam.has(key)) fam.set(key, { name: fk.key, dir: P.dirname(f), members: [], marked: false }); const e = fam.get(key); e.members.push(f); e.marked ||= fk.marked; }
  const keepSet = new Set((opts.keep || []).map((k) => k.replace(/\\/g, '/')));
  const decided = new Set();
  for (const [key, e] of fam) {
    const distinct = new Set(e.members.map((m) => P.basename(m)));
    if (distinct.size < 2 || !e.marked) continue;
    const members = e.members.filter((m) => !moves.has(m));
    if (members.length < 2) continue;
    const kept = members.find((m) => keepSet.has(m));
    if (kept) { for (const m of members) if (m !== kept && movable(m) && DOC_EXT.has(P.extname(m).toLowerCase())) { moves.set(m, archiveOf(m)); reasons.set(m, [`older version: owner kept ${kept}`, 'high']); } members.filter((m) => m !== kept).forEach((m) => decided.add(m)); continue; }
    res.decisions.push({ question: `Which one is current? (version family "${e.name}"${e.dir === '.' ? '' : ` in ${e.dir}/`})`, candidates: members.sort((a, b) => (stat.get(b)?.mtimeMs || 0) - (stat.get(a)?.mtimeMs || 0)), how: members.every((m) => DOC_EXT.has(P.extname(m).toLowerCase())) ? 're-run survey with --keep <path>; the others are then archived' : 'code is not filed: keep one in git, delete the rest after references/carry-forward.md' });
    members.forEach((m) => decided.add(m));
  }
  const roleDocs = new Map();
  for (const f of mdDocs) { if (f.startsWith(`${L.archive}/`) || moves.has(f) || decided.has(f) || FRONT_DOORS.has(P.basename(f).toLowerCase())) continue; const m = ROLE.exec(P.basename(f)); if (m) { const r = m[1].toLowerCase().replace(/[ _-]/g, ''); if (!roleDocs.has(r)) roleDocs.set(r, []); roleDocs.get(r).push(f); } }
  for (const [role, list] of roleDocs) if (list.length > 1 && !list.some((m) => keepSet.has(m))) res.decisions.push({ question: `${list.length} docs compete as the "${role}" doc. Which is true now?`, candidates: list, how: `name it in ${L.current}; archive the others with "archive <old> --by <new>"` });
  // folders whose names say "old": the owner decides; nothing moves on a folder name alone
  const OLD_DIR = /^(old|old[ _-].*|.*[ _-]old|archived?|backups?|bak|deprecated|legacy|unused|attic|trash)$/i;
  for (const d of [...ctx.dirSet].sort()) {
    if (!OLD_DIR.test(P.basename(d)) || d === L.archive || d.startsWith(`${L.archive}/`) || inBuild(`${d}/x`) || [...ctx.dirSet].some((x) => x !== d && d.startsWith(`${x}/`) && OLD_DIR.test(P.basename(x)))) continue;
    const inside = ctx.files.filter((f) => f.startsWith(`${d}/`));
    res.decisions.push({ question: `Folder "${d}/" is named as old (${inside.length} files). Superseded?`, candidates: inside.slice(0, 8), how: `documents: "archive <file> --by <replacement>" one by one (byte for byte); code: references/carry-forward.md` });
  }
  // loose root documents go under docs/
  for (const f of ctx.files) {
    if (f.includes('/') || !DOC_EXT.has(P.extname(f).toLowerCase()) || ROOT_KEEP.has(f.toLowerCase()) || moves.has(f) || !movable(f) || decided.has(f)) continue;
    const to = `${L.docs}/${f}`;
    if (lower.has(to.toLowerCase())) continue;
    moves.set(f, to); reasons.set(f, ['loose document at the top level', 'medium']);
  }
  // target collisions (case-insensitive) — never overwrite
  const taken = new Set([...ctx.files, ...ctx.dirSet].map((x) => x.toLowerCase()));
  for (const [from, to] of [...moves]) { if (taken.has(to.toLowerCase())) { moves.delete(from); res.entries.push({ from, action: 'stay', reason: `target ${to} already exists`, confidence: 'high' }); } else taken.add(to.toLowerCase()); }
  for (const [from, to] of moves) res.entries.push({ from, to, action: 'move', reason: reasons.get(from)[0], confidence: reasons.get(from)[1], hash: hashes.get(from) || hashFile(absOf(root, from)), tracked: tracked.has(from), ...(redirect.has(from) ? { redirect: redirect.get(from) } : {}) });
  for (const p of res.pinned) res.entries.push({ from: p.path, action: 'stay', reason: `pinned: referenced by ${p.by.join(', ')}`, confidence: 'high' });
  for (const f of dirty) res.entries.push({ from: f, action: 'stay', reason: 'uncommitted changes in git: commit first', confidence: 'high' });
  // stale and broken links across every markdown doc
  const an = analyzeDocs(ctx, mdDocs, { noEdited: true });
  res.brokenLinks = an.fail.map((x) => ({ path: x.path, problem: x.msg }));
  res.stale = an.warn.map((x) => ({ path: x.path, problem: x.msg }));
  res.patterns.families = 'names that differ only by (n), copy, final, real, latest, new, old, backup, bak, draft, wip, updated, revised, edited, fixed, vN, dates';
  res.patterns.roles = ROLE.source;
  res.summary = { move: res.entries.filter((e) => e.action === 'move').length, pinned: res.pinned.length, decisions: res.decisions.length, duplicates: res.duplicates.length, secrets: res.secrets.length, hazards: res.hazards.length, broken: res.brokenLinks.length, stale: res.stale.length };
  return res;
}
export function planMarkdown(plan) {
  const s = plan.summary; const L = [];
  const tbl = (head, rows) => (rows.length ? [head, `|${head.split('|').slice(1, -1).map(() => '---').join('|')}|`, ...rows] : ['- none (pattern searched; 0 matches)']);
  L.push('# Reorganisation plan (review before apply)', '', `${plan.files} files surveyed (${plan.git ? 'git: tracked + untracked, gitignored excluded' : 'no git: .gitignore honoured'}). Proposed moves: ${s.move}. Nothing is deleted.`, '');
  L.push('## Owner decisions needed', '', ...(plan.decisions.length ? plan.decisions.flatMap((d) => [`- **${d.question}** ${d.how}`, ...d.candidates.map((c) => `  - \`${c}\``)]) : ['- none']), '');
  L.push('## Proposed moves', '', ...tbl('| From | To | Why | Confidence |', plan.entries.filter((e) => e.action === 'move').map((e) => `| \`${e.from}\` | \`${e.to}\` | ${e.reason} | ${e.confidence} |`)), '');
  L.push('## Pinned (stay where they are)', '', ...tbl('| File | Referenced by |', plan.pinned.map((p) => `| \`${p.path}\` | ${p.by.join(', ')} |`)), '');
  L.push('## Secret-shaped files (names only; contents never read)', '', ...(plan.secrets.length ? plan.secrets.map((x) => `- \`${x.path}\`${x.tracked ? ' — **TRACKED by git**' : ''} — ${x.note}`) : ['- none']), '');
  L.push('## Transfers and backups (zip, bundle, tarball)', '', ...(plan.transfers.length ? plan.transfers.map((x) => `- \`${x.path}\` — ${x.note}`) : ['- none']), '');
  L.push('## Build outputs present (left alone)', '', ...(plan.builds.length ? plan.builds.map((b) => `- \`${b}/\``) : ['- none']), ...(plan.ignoredPresent.length ? ['', `Gitignored, not surveyed: ${plan.ignoredPresent.map((x) => `\`${x}\``).join(', ')}`] : []), '');
  L.push('## Windows hazards', '', ...(plan.hazards.length ? plan.hazards.map((h) => `- \`${h.path}\` — ${h.problem}`) : ['- none']), '');
  L.push('## Broken links', '', ...(plan.brokenLinks.length ? plan.brokenLinks.map((h) => `- \`${h.path}\` — ${h.problem}`) : ['- none']), '');
  L.push('## Stale docs', '', ...(plan.stale.length ? plan.stale.map((h) => `- \`${h.path}\` — ${h.problem}`) : ['- none']), '');
  L.push('## Patterns searched (a "none" above means these matched nothing, not that nothing exists)', '', ...Object.entries(plan.patterns).map(([k, v]) => `- ${k}: \`${v}\``), ...(plan.limits.length ? plan.limits.map((x) => `- LIMIT: ${x}`) : []), `- CRLF markdown docs: ${plan.crlfDocs}`, ...(plan.cloudOnly.length ? [`- possibly cloud-only (not read): ${plan.cloudOnly.length}`] : []), '');
  return L.join('\n');
}

// ---------------------------------------------------------------- apply / undo (journaled, reversible)
function stageGit(root, paths) {
  if (!paths.length) return;
  const r = git(root, ['add', '-A', '--pathspec-from-file=-', '--pathspec-file-nul'], { input: paths.map((p) => `:(literal)${p}`).join('\0') });
  if (!r.ok) throw new Fail(`git add failed: ${r.err.trim()}`);
}
function moveFile(root, from, to, trashDir) {
  const a = absOf(root, from); const b = absOf(root, to);
  try {
    if (process.env.FILING_TEST_EXDEV === '1') throw Object.assign(new Error('EXDEV (test)'), { code: 'EXDEV' });
    retry(() => fs.renameSync(a, b)); return { mode: 'rename' };
  } catch (e) {
    if (e.code !== 'EXDEV') throw e;
    fs.copyFileSync(a, b); const st = fs.statSync(a); fs.utimesSync(b, st.atime, st.mtime);
    if (hashFile(a) !== hashFile(b)) { fs.unlinkSync(b); throw new Fail(`copy of ${from} did not verify; left in place`); }
    const t = path.join(trashDir, ...from.split('/')); fs.mkdirSync(path.dirname(t), { recursive: true });
    try { retry(() => fs.renameSync(a, t)); return { mode: 'copy', trash: relPosix(root, t) }; } catch { return { mode: 'copy-kept' }; }
  }
}
export function apply(root, plan, opts = {}) {
  const ctx = context(root); ctx.dirSet = dirSetOf(ctx.files);
  const moves = plan.entries.filter((e) => e.action === 'move');
  const pinned = new Set((plan.pinned || []).map((p) => p.path));
  const problems = [];
  const taken = new Set([...ctx.files, ...ctx.dirSet].map((x) => x.toLowerCase()));
  for (const m of moves) {
    if (pinned.has(m.from)) problems.push(`${m.from} is pinned (referenced by code or a scheduled task); refusing to move it`);
    if (!ctx.fileSet.has(m.from)) problems.push(`${m.from} no longer exists`);
    else if (m.hash && hashFile(absOf(root, m.from)) !== m.hash) problems.push(`${m.from} changed since the survey; re-run survey`);
    if (taken.has(m.to.toLowerCase())) problems.push(`${m.to} already exists (case-insensitive); refusing to overwrite`);
    taken.add(m.to.toLowerCase());
  }
  if (problems.length) throw new Fail(`apply refused, nothing changed:\n  ${problems.join('\n  ')}`);
  const map = new Map(moves.map((m) => [m.from, m.to]));
  const linkMap = new Map(moves.map((m) => [m.from, m.redirect ? map.get(m.redirect) || m.redirect : m.to]));
  const existed = (p) => ctx.fileSet.has(p) || ctx.dirSet.has(p.replace(/\/$/, ''));
  const edits = [];
  for (const f of ctx.files) {
    if (!/\.(md|markdown)$/i.test(f) || f.startsWith('.filing/')) continue;
    const buf = fs.readFileSync(absOf(root, f)); if (buf.length > 4 << 20) continue;
    const before = buf.toString('utf8'); const newRel = map.get(f) || f;
    const after = rewriteLinks(before, f, newRel, linkMap, existed);
    if (after !== before) edits.push({ path: newRel, origPath: f, before: buf, after: guardedRewrite(before, after, f) });
  }
  if (!opts.yes) return { dryRun: true, moves, edits: edits.map((e) => e.path) };
  const filingExisted = exists(path.join(root, '.filing'));
  return withLease(root, 'apply', (lease) => {
    const stamp = new Date().toISOString().replace(/[-:]/g, '').replace(/\..*/, 'Z') + `-${rand(2)}`;
    const jdir = path.join(root, '.filing', 'journal', stamp); fs.mkdirSync(path.join(jdir, 'orig'), { recursive: true });
    const J = path.join(jdir, 'journal.jsonl');
    const log = (o) => { const fd = fs.openSync(J, 'a'); fs.writeSync(fd, `${JSON.stringify(o)}\n`); fs.fsyncSync(fd); fs.closeSync(fd); };
    log({ op: 'begin', version: 1, filingExisted, journalCreated: true });
    edits.forEach((e, i) => { fs.writeFileSync(path.join(jdir, 'orig', String(i)), e.before); const st = fs.statSync(absOf(root, e.origPath)); log({ op: 'edit', i, path: e.path, beforeHash: sha(e.before), afterHash: sha(Buffer.from(e.after)), mtime: st.mtimeMs, atime: st.atimeMs }); });
    const trash = path.join(root, '.filing', 'trash', stamp);
    const tracked = [];
    moves.forEach((m, i) => {
      const parts = m.to.split('/');
      for (let k = 1; k < parts.length; k++) { const d = parts.slice(0, k).join('/'); if (!exists(absOf(root, d))) { log({ op: 'mkdir', path: d }); fs.mkdirSync(absOf(root, d)); } }
      log({ op: 'move', i, from: m.from, to: m.to, hash: m.hash, tracked: !!m.tracked });
      const r = moveFile(root, m.from, m.to, trash);
      log({ op: 'moved', i, ...r });
      if (+process.env.FILING_TEST_CRASH_AFTER === i + 1) process.kill(process.pid, 'SIGKILL'); // test hook: die mid-apply
      if (m.tracked) tracked.push(m.from, m.to);
      if (i % 50 === 49) lease.renew();
    });
    if (ctx.git && tracked.length) { stageGit(root, tracked); log({ op: 'staged', paths: tracked }); }
    edits.forEach((e, i) => { writeAtomic(absOf(root, e.path), e.after); log({ op: 'edited', i }); });
    log({ op: 'end' });
    // references in non-markdown text that we did not edit
    const unedited = [];
    for (const f of ctx.files) {
      if (!TEXT_EXT.has(P.extname(f).toLowerCase()) || /\.(md|markdown)$/i.test(f) || SECRET_NAME.test(P.basename(f))) continue;
      const t = readText(absOf(root, map.get(f) || f)); if (!t) continue;
      for (const m of moves) if (t.includes(m.from) || t.includes(m.from.replace(/\//g, '\\'))) unedited.push(`${map.get(f) || f} mentions ${m.from}`);
    }
    return { journal: relPosix(root, jdir), moved: moves.length, edited: edits.map((e) => e.path), unedited };
  });
}
function latestJournal(root) {
  const d = path.join(root, '.filing', 'journal');
  const all = exists(d) ? fs.readdirSync(d).filter((x) => exists(path.join(d, x, 'journal.jsonl'))).sort() : [];
  if (!all.length) throw new Fail('no apply journal found in .filing/journal');
  return path.join(d, all[all.length - 1]);
}
export function undo(root, journalDir) {
  const jdir = journalDir ? path.resolve(root, journalDir) : latestJournal(root);
  const recs = fs.readFileSync(path.join(jdir, 'journal.jsonl'), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l));
  const begin = recs.find((r) => r.op === 'begin');
  const edits = recs.filter((r) => r.op === 'edit'); const editedDone = new Set(recs.filter((r) => r.op === 'edited').map((r) => r.i));
  const moves = recs.filter((r) => r.op === 'move'); const movedRec = new Map(recs.filter((r) => r.op === 'moved').map((r) => [r.i, r]));
  const mkdirs = recs.filter((r) => r.op === 'mkdir');
  const staged = recs.find((r) => r.op === 'staged');
  const editAt = new Map(edits.map((e) => [e.path, e]));
  // verify first: refuse if anything changed since apply
  const problems = [];
  const cur = (rel) => (isFile(absOf(root, rel)) ? hashFile(absOf(root, rel)) : null);
  for (const e of edits) { if (!editedDone.has(e.i)) continue; const h = cur(e.path); if (h !== e.afterHash) problems.push(`${e.path} was edited after apply`); }
  for (const m of moves) {
    const done = movedRec.has(m.i) || (cur(m.to) && !exists(absOf(root, m.from)));
    if (!done) continue;
    const e = editAt.get(m.to); const want = e && editedDone.has(e.i) ? e.afterHash : m.hash;
    if (want && cur(m.to) !== want) problems.push(`${m.to} changed after apply`);
    if (exists(absOf(root, m.from)) && (movedRec.get(m.i) || {}).mode !== 'copy-kept') problems.push(`${m.from} exists again; refusing to overwrite it`);
  }
  if (problems.length) throw new Fail(`undo refused, nothing changed:\n  ${problems.join('\n  ')}`);
  const res = withLease(root, 'undo', () => {
    for (const e of edits) {
      if (!editedDone.has(e.i) && cur(e.path) !== e.afterHash) continue;
      const a = absOf(root, e.path); writeAtomic(a, fs.readFileSync(path.join(jdir, 'orig', String(e.i)))); fs.utimesSync(a, new Date(e.atime), new Date(e.mtime));
    }
    for (const m of [...moves].reverse()) {
      const r = movedRec.get(m.i) || (cur(m.to) && !exists(absOf(root, m.from)) ? { mode: 'rename' } : null);
      if (!r) continue;
      if (r.mode === 'rename') retry(() => fs.renameSync(absOf(root, m.to), absOf(root, m.from)));
      else if (r.mode === 'copy') { retry(() => fs.renameSync(absOf(root, r.trash), absOf(root, m.from))); fs.unlinkSync(absOf(root, m.to)); }
      else if (r.mode === 'copy-kept') fs.unlinkSync(absOf(root, m.to));
    }
    if (staged) stageGit(root, staged.paths);
    for (const d of [...mkdirs].reverse()) { try { fs.rmdirSync(absOf(root, d.path)); } catch { /* not empty: something else lives there now */ } }
    const leftovers = [];
    const rm = (p) => { try { fs.rmSync(p, { recursive: true, force: true }); } catch { leftovers.push(relPosix(root, p)); } };
    rm(jdir);
    const trash = path.join(root, '.filing', 'trash', path.basename(jdir)); if (exists(trash)) rm(trash);
    for (const d of ['trash', 'journal']) { const p = path.join(root, '.filing', d); try { if (!fs.readdirSync(p).length) fs.rmdirSync(p); } catch { /* absent */ } }
    return { restored: moves.length, edits: edits.length, filingExisted: begin && begin.filingExisted, leftovers };
  });
  cleanupFilingIfCreated(root, res);
  return res;
}
function cleanupFilingIfCreated(root, info) {
  if (info.filingExisted) return;
  const p = path.join(root, '.filing');
  try { const left = fs.readdirSync(p).filter((x) => !x.startsWith('lease')); if (!left.length) fs.rmSync(p, { recursive: true, force: true }); } catch { /* absent */ }
}

// ---------------------------------------------------------------- archive (supersede one doc, byte for byte)
/**
 * True when `after` differs from `before` only in the targets of markdown links (path tokens). Used as a guard:
 * a reorganisation may repoint links, and must never change any other byte of a doc.
 */
export function onlyLinkPathsDiffer(before, after) {
  const mask = (t) => mdSegments(lf(t)).map((seg) => (seg.code ? seg.s : seg.s.replace(LINK_RE, (w, a1, t1, a3, d1) => (t1 ? `${a1}\u0000${a3}` : `${d1}\u0000`)))).join('');
  return mask(before) === mask(after);
}
function guardedRewrite(before, after, where) {
  if (!onlyLinkPathsDiffer(before, after)) throw new Fail(`refusing to edit ${where}: the change is more than link paths (a reorganisation never edits content)`);
  return after;
}
export function archive(root, file, by) {
  if (!by) throw new Fail('archive needs --by "<what replaces it>"', 2);
  const ctx = context(root); ctx.dirSet = dirSetOf(ctx.files);
  const rel = relPosix(root, path.resolve(root, file));
  if (!ctx.fileSet.has(rel)) throw new Fail(`no such file: ${rel}`);
  const L = ctx.cfg.layout; const day = today();
  const ext = P.extname(rel); const stem = P.basename(rel, ext);
  let dest; for (let n = 1; ; n++) { dest = `${L.archive}/${stem}_ARCHIVED_${day}${n > 1 ? `_${n}` : ''}${ext}`; if (!exists(absOf(root, dest))) break; }
  return withLease(root, 'archive', () => {
    fs.mkdirSync(path.dirname(absOf(root, dest)), { recursive: true });
    const tracked = ctx.git && git(root, ['ls-files', '--error-unmatch', '--', `:(literal)${rel}`]).ok;
    const hash = hashFile(absOf(root, rel));
    retry(() => fs.renameSync(absOf(root, rel), absOf(root, dest))); // byte for byte: the superseded doc is never edited
    if (tracked) stageGit(root, [rel, dest]);
    const idx = absOf(root, `${L.archive}/INDEX.tsv`);
    if (!exists(idx)) fs.writeFileSync(idx, 'archived\tfrom\tto\tsuperseded_by\twho\tsha256\n');
    fs.appendFileSync(idx, `${day}\t${rel}\t${dest}\t${by.replace(/[\t\r\n]/g, ' ')}\t${who()}\t${hash}\n`);
    // links pointing at the old doc: repoint them (link paths only) at the replacement when it is a file, else report
    const relinked = []; const report = [];
    const target = ctx.fileSet.has(by) ? by : null;
    for (const d of ctx.files) {
      if (d === rel || !/\.(md|markdown)$/i.test(d) || d.startsWith(`${L.archive}/`)) continue;
      const t = readText(absOf(root, d)); if (!t || !mdLinks(t).some((l) => P.normalize(P.join(P.dirname(d), l)) === rel)) continue;
      if (target) { writeAtomic(absOf(root, d), guardedRewrite(t, rewriteLinks(t, d, d, new Map([[rel, target]]), (p) => ctx.fileSet.has(p) || ctx.dirSet.has(p)), d)); relinked.push(d); } else report.push(d);
    }
    appendEvent(root, { op: 'archive', from: rel, to: dest, by });
    renderAll(context(root));
    return { dest, relinked, report };
  });
}

// ---------------------------------------------------------------- refs: merged by content, not ancestry
export function refs(root, mainOpt) {
  if (!isGit(root)) throw new Fail('refs needs a git repository');
  const cfg = loadConfig(root);
  const heads = git(root, ['for-each-ref', '--format=%(refname:short)\t%(objectname)', 'refs/heads']).out.trim().split('\n').filter(Boolean).map((l) => l.split('\t'));
  const names = heads.map((h) => h[0]);
  const main = mainOpt || cfg.main || ['main', 'master'].find((m) => names.includes(m)) || git(root, ['symbolic-ref', '--short', '-q', 'HEAD']).out.trim();
  if (!main) throw new Fail('no main branch found; pass --main <branch>');
  const mainTree = git(root, ['rev-parse', `${main}^{tree}`]).out.trim();
  const out = { main, merged: [], unmerged: [], unknown: [], worktrees: [] };
  for (const [name, sha1] of heads) {
    if (name === main) continue;
    let how = null;
    if (git(root, ['merge-base', '--is-ancestor', sha1, main]).ok) how = 'ancestor';
    else {
      const ch = git(root, ['cherry', main, sha1]);
      if (ch.ok && ch.out.trim() && ch.out.trim().split('\n').every((l) => l.startsWith('-'))) how = 'every commit has an equivalent patch in main (rebase merge)';
      else {
        const mt = git(root, ['merge-tree', '--write-tree', main, sha1]);
        if (mt.ok && mt.out.split('\n')[0].trim() === mainTree) how = 'merging it would change nothing (squash merge)';
        else if (!mt.ok && /unknown option|usage/i.test(mt.err)) { out.unknown.push({ name, why: 'git too old for merge-tree --write-tree; KEEP' }); continue; }
      }
    }
    (how ? out.merged : out.unmerged).push({ name, sha: sha1.slice(0, 10), ...(how ? { how } : {}) });
  }
  let wt = null;
  for (const l of git(root, ['worktree', 'list', '--porcelain']).out.split('\n')) {
    if (l.startsWith('worktree ')) { wt = { path: relPosix(root, l.slice(9)) || '.' }; out.worktrees.push(wt); }
    else if (l.startsWith('branch ') && wt) wt.branch = l.slice(7).replace('refs/heads/', '');
  }
  return out;
}

// ---------------------------------------------------------------- status (live) and close
function gitState(root, fetch) {
  if (!isGit(root)) return null;
  const s = { branch: git(root, ['rev-parse', '--abbrev-ref', 'HEAD']).out.trim(), sha: git(root, ['rev-parse', '--short=10', 'HEAD']).out.trim() || '(no commits)' };
  s.dirtyFiles = git(root, ['status', '--porcelain']).out.split('\n').filter(Boolean);
  if (fetch) {
    const hasRemote = git(root, ['remote']).out.trim();
    if (!hasRemote) s.fetch = 'skipped (no remote)';
    else { const f = git(root, ['fetch', '--quiet'], { timeout: 10000 }); s.fetch = f.ok ? 'ok' : `FAILED (${(f.err || 'timeout').trim().split('\n')[0]}) — ahead/behind is UNVERIFIED`; }
  } else s.fetch = 'skipped';
  const ab = git(root, ['rev-list', '--left-right', '--count', '@{u}...HEAD']);
  s.aheadBehind = ab.ok ? (() => { const [b, a] = ab.out.trim().split(/\s+/); return `${a} ahead, ${b} behind upstream`; })() : 'no upstream';
  return s;
}
function status(root) {
  const ctx = context(root); const st = fold(readLedger(root)); const g = gitState(root, false);
  const cur = readText(absOf(root, ctx.cfg.layout.current)) || '';
  const curLine = currentLine(cur) || summaryOf(cur);
  const lines = [`current (${ctx.cfg.layout.current}, ${declaredDate(cur) || 'UNDATED'}): ${curLine || 'MISSING'}`];
  if (g) lines.push(`git: ${g.branch} @ ${g.sha}; ${g.dirtyFiles.length} uncommitted; ${g.aheadBehind} (as of last fetch; re-verify: git fetch && git status -sb)`);
  else lines.push('git: not a git repository');
  const last = st.closes[st.closes.length - 1];
  lines.push(`next: ${last ? last.next : 'no session closed yet'}`);
  for (const s of ['doing', 'open']) { const l = [...st.items.values()].filter((i) => i.status === s); lines.push(`${s} (${l.length}):`, ...l.slice(0, 15).map((i) => `  ${i.id}  ${i.title}${i.home ? `  [${i.home}]` : ''}${i.pkg ? `  (${i.pkg})` : ''}`)); }
  let lease = null; try { lease = JSON.parse(readText(path.join(root, '.filing', 'lease.json'))); } catch { /* none */ }
  if (lease && !leaseDead(lease)) lines.push(`lease: held by ${lease.holder} (${lease.cmd})`);
  let lane = null; try { lane = JSON.parse(readText(path.join(root, '.filing', 'lane.json'))); } catch { /* none */ }
  if (lane && lane.holder) lines.push(`merge lane: ${lane.holder} — ${lane.why} (since ${lane.since})`);
  return lines.join('\n');
}
export function close(root, summary, o) {
  if (!summary || !summary.trim()) throw new Fail('close needs a summary: close "<what was done>" --next "<exact command>"', 2);
  if (!o.next || !o.next.trim()) throw new Fail('close needs --next "<the exact next command>", not "continue X"', 2);
  const cfg = loadConfig(root); const g = gitState(root, !o.noFetch);
  return withLease(root, 'close', () => {
    const day = today(); const slug = summary.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 40) || 'session';
    let rel; for (let n = 1; ; n++) { rel = `${cfg.layout.sessions}/${day}-${slug}${n > 1 ? `-${n}` : ''}.md`; if (!exists(absOf(root, rel))) break; }
    const list = (a, pre = '') => (a && a.length ? a.map((x) => `- ${pre}${x}`) : ['- none recorded']);
    const body = [`# Session ${day}: ${summary}`, '', `Last updated: ${day}`, `By: ${who()}`, '',
      '## Git at close', '', g ? `- ${g.branch} @ ${g.sha}; ${g.dirtyFiles.length} uncommitted file(s); ${g.aheadBehind}; fetch: ${g.fetch}` : '- not a git repository',
      ...(g && g.dirtyFiles.length ? ['', '```', ...g.dirtyFiles.slice(0, 30), '```'] : []), '',
      '## Summary', '', summary, '', '## Next', '', '```', o.next, '```', '',
      '## Verified', '', ...list(o.verified), '', '## Unverified', '', ...list(o.unverified, 'UNVERIFIED: '), '', '## Blockers', '', ...list(o.blockers), ''].join('\n');
    writeAtomic(absOf(root, rel), body);
    appendEvent(root, { op: 'close', summary, next: o.next, verified: o.verified || [], unverified: o.unverified || [], blockers: o.blockers || [],
      branch: g ? g.branch : '', sha: g ? g.sha : '', dirty: g ? g.dirtyFiles.length : 0, fetch: g ? g.fetch : 'n/a', session: rel });
    renderAll(context(root));
    return rel;
  });
}

// ---------------------------------------------------------------- items
export function item(root, sub, args, o) {
  return withLease(root, `item ${sub}`, () => itemLocked(root, sub, args, o));
}
function itemLocked(root, sub, args, o) {
  const st = fold(readLedger(root));
  const need = (id) => { if (!st.items.has(id)) throw new Fail(`no item ${id}`); return st.items.get(id); };
  let ev;
  if (sub === 'add') {
    const title = args.join(' ').trim(); if (!title) throw new Fail('usage: item add "<title>" [--id X] [--home path] [--pkg dir] [--files a,b] [--owner X]', 2);
    let id = o.id; if (!id) do { id = `i${rand(2)}`; } while (st.items.has(id));
    if (st.items.has(id)) throw new Fail(`item ${id} already exists`);
    if (o.home && !exists(path.resolve(root, o.home))) throw new Fail(`home "${o.home}" does not exist; create it first or omit --home`);
    ev = { op: 'add', id, title, ...(o.home ? { home: relPosix(root, path.resolve(root, o.home)) } : {}), ...(o.pkg ? { pkg: o.pkg } : {}), ...(o.files ? { files: o.files.split(',').map((x) => x.trim()) } : {}), ...(o.owner ? { owner: o.owner } : {}) };
  } else if (['start', 'done', 'reopen'].includes(sub)) { need(args[0]); ev = { op: sub, id: args[0] }; }
  else if (sub === 'drop') { need(args[0]); if (!o.why) throw new Fail('drop needs --why "<reason>"', 2); ev = { op: 'drop', id: args[0], why: o.why }; }
  else if (sub === 'note') { need(args[0]); const text = args.slice(1).join(' ').trim(); if (!text) throw new Fail('usage: item note <id> "<text>"', 2); ev = { op: 'note', id: args[0], text }; }
  else if (sub === 'set') { need(args[0]); ev = { op: 'set', id: args[0] }; for (const k of ['title', 'home', 'pkg', 'owner']) if (o[k]) ev[k] = o[k]; if (o.files) ev.files = o.files.split(',').map((x) => x.trim()); }
  else throw new Fail('usage: item add|start|done|drop|note|set|reopen', 2);
  appendEvent(root, ev); const ch = renderAll(context(root)); return { id: ev.id, changed: ['.filing/ledger.jsonl', ...ch] };
}

function lessonLocked(root, sub, args, o) {
  const st = fold(readLedger(root)); const act = st.lessons.filter((l) => !l.retired);
  if (sub === 'list') return { text: act.length ? act.map((l) => `${l.id}  ${l.wrong} -> ${l.instead}`).join('\n') : 'no standing corrections', changed: [] };
  let ev;
  if (sub === 'add') {
    const wrong = args.join(' ').trim(); const instead = String(o.do || '').trim();
    if (!wrong || !instead) throw new Fail('usage: lesson add "<what went wrong>" --do "<what to do instead>"', 2);
    if (wrong.split(/\s+/).length > 30 || instead.split(/\s+/).length > 30) throw new Fail('keep each side to 30 words: one correction, one line', 2);
    const dup = act.find((l) => l.wrong.toLowerCase() === wrong.toLowerCase() && l.instead.toLowerCase() === instead.toLowerCase());
    if (dup) return { text: `already recorded as ${dup.id}`, changed: [] };
    let id; do { id = `L${rand(2)}`; } while (st.lessons.some((l) => l.id === id));
    ev = { op: 'lesson', id, wrong, instead };
  } else if (sub === 'retire') {
    if (!act.some((l) => l.id === args[0])) throw new Fail(`no active correction ${args[0]}`);
    if (!o.why) throw new Fail('retire needs --why "<reason>"', 2);
    ev = { op: 'unlesson', id: args[0], why: o.why };
  } else throw new Fail('usage: lesson add|retire|list', 2);
  appendEvent(root, ev); const ch = renderAll(context(root));
  return { text: `${sub} ${ev.id}: wrote ${['.filing/ledger.jsonl', ...ch].join(', ')}`, changed: ch };
}

// ---------------------------------------------------------------- lane (one merger at a time on main)
function lane(root, sub, args, o) {
  const f = path.join(root, '.filing', 'lane.json');
  let cur = null; try { cur = JSON.parse(readText(f)); } catch { /* free */ }
  if (!sub || sub === 'show') return cur && cur.holder ? `held by ${cur.holder}: ${cur.why} (since ${cur.since})` : 'free';
  return withLease(root, `lane ${sub}`, () => {
    if (sub === 'claim') {
      if (cur && cur.holder && cur.holder !== who() && !o.force) throw new Fail(`lane held by ${cur.holder}: ${cur.why}. Wait, or --force if they are gone (it is logged).`);
      writeAtomic(f, JSON.stringify({ holder: who(), why: args.join(' ') || '(no reason)', since: new Date().toISOString() }));
      return `lane claimed by ${who()}`;
    }
    if (sub === 'release') { if (cur && cur.holder && cur.holder !== who() && !o.force) throw new Fail(`lane is held by ${cur.holder}, not ${who()}`); writeAtomic(f, JSON.stringify({ holder: '' })); return 'lane released'; }
    throw new Fail('usage: lane [show|claim <why>|release] [--force]', 2);
  });
}

// ---------------------------------------------------------------- init
export function detectPackages(root) {
  const pk = new Set(); const globs = [];
  try { const j = JSON.parse(readText(path.join(root, 'package.json'))); const w = j.workspaces; globs.push(...(Array.isArray(w) ? w : (w && w.packages) || [])); } catch { /* none */ }
  const pn = readText(path.join(root, 'pnpm-workspace.yaml')); if (pn) for (const m of pn.matchAll(/^\s*-\s*['"]?([^'"\n#]+?)['"]?\s*$/gm)) globs.push(m[1]);
  for (const f of ['Cargo.toml', 'pyproject.toml']) { const t = readText(path.join(root, f)); const m = t && /members\s*=\s*\[([^\]]*)\]/.exec(t); if (m) for (const x of m[1].matchAll(/["']([^"']+)["']/g)) globs.push(x[1]); }
  for (const g of globs) {
    if (g.startsWith('!')) continue;
    const clean = g.replace(/\/+$/, '').replace(/^\.\//, '');
    if (!clean.includes('*')) { if (exists(absOf(root, clean))) pk.add(clean); continue; }
    const base = clean.slice(0, clean.indexOf('*')).replace(/\/$/, ''); const re = globRe(clean);
    try { for (const e of fs.readdirSync(absOf(root, base || '.'), { withFileTypes: true })) { const rel = base ? `${base}/${e.name}` : e.name; if (e.isDirectory() && re.test(rel)) pk.add(rel); } } catch { /* none */ }
  }
  return [...pk].sort();
}
function tpl(name, vars) {
  const dir = path.join(path.dirname(SELF), '..', 'templates');
  const t = readText(path.join(dir, name));
  if (t == null) throw new Fail(`template ${name} not found next to this script; run init from the skill folder, not the vendored copy`);
  return lf(t).replace(/\{\{(\w+)\}\}/g, (_, k) => (k in vars ? vars[k] : `{{${k}}}`));
}
export function upsertBlock(text, block) {
  if (text == null) return null;
  const b = text.indexOf(BLOCK_BEGIN); const e = text.indexOf(BLOCK_END);
  if (b >= 0 && e > b) return text.slice(0, b) + block.trimEnd() + text.slice(e + BLOCK_END.length);
  return `${block.trimEnd()}\n\n${text}`;
}
function vendoredVersion(file) { const m = /export const VERSION = '([\d.]+)'/.exec(readText(file) || ''); return m ? m[1] : null; }
const verLess = (a, b) => { const x = a.split('.').map(Number); const y = b.split('.').map(Number); for (let i = 0; i < 3; i++) if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) < (y[i] || 0); return false; };

export function init(root, o = {}) {
  root = path.resolve(root);
  if (+process.versions.node.split('.')[0] < 18) throw new Fail(`Node ${process.versions.node} is too old; install Node 18+ (a portable zip from nodejs.org needs no admin rights)`);
  const gitRepo = isGit(root);
  const hasStuff = ['README.md', 'docs', 'src', 'package.json', 'pyproject.toml'].some((x) => exists(path.join(root, x)));
  const mode = o.mode || (hasStuff ? 'adopt' : 'new');
  const cfgFile = path.join(root, '.filing', 'config.json');
  let cfg;
  if (exists(cfgFile)) cfg = loadConfig(root);
  else {
    cfg = JSON.parse(JSON.stringify(DEFAULTS));
    if (o.preset === 'numbered') {
      cfg.layout = { docs: '01_DOCS', current: '00_SOURCE_OF_TRUTH/STATUS.md', index: '00_SOURCE_OF_TRUTH/INDEX.md', archive: '04_ARCHIVE', sessions: '02_SESSION_LOG' };
      cfg.docGlobs = ['*.md', '0[0-9]_*/**/*.md']; cfg.docExclude = ['04_ARCHIVE/**', '02_SESSION_LOG/**'];
    } else if (mode === 'adopt' && !exists(path.join(root, 'docs'))) {
      const alt = ['doc', 'documentation'].find((d) => exists(path.join(root, d)));
      if (alt) { cfg.layout = { docs: alt, current: `${alt}/STATUS.md`, index: `${alt}/INDEX.md`, archive: `${alt}/archive`, sessions: `${alt}/sessions` }; cfg.docGlobs = ['*.md', `${alt}/**/*.md`]; cfg.docExclude = [`${alt}/archive/**`, `${alt}/sessions/**`]; }
    }
    cfg.packages = detectPackages(root);
    for (const p of cfg.packages) cfg.docGlobs.push(`${p}/*.md`, `${p}/docs/**/*.md`);
    if (o.owner) cfg.owner = o.owner;
  }
  const L = cfg.layout; const actions = [];
  const plan = (rel, what, fn) => actions.push({ rel, what, fn });
  const at = (rel) => absOf(root, rel);
  // vendored script
  const vend = at('.filing/filing.mjs'); const vv = vendoredVersion(vend);
  if (!vv) plan('.filing/filing.mjs', 'create', () => { fs.mkdirSync(path.dirname(vend), { recursive: true }); fs.copyFileSync(SELF, vend); });
  else if (verLess(vv, VERSION)) plan('.filing/filing.mjs', `upgrade ${vv} -> ${VERSION}`, () => fs.copyFileSync(SELF, vend));
  else actions.push({ rel: '.filing/filing.mjs', what: `keep (version ${vv})` });
  if (!exists(cfgFile)) plan('.filing/config.json', 'create', () => writeAtomic(cfgFile, `${JSON.stringify(cfg, null, 2)}\n`));
  if (!exists(at('.filing/ledger.jsonl'))) plan('.filing/ledger.jsonl', 'create (empty)', () => fs.writeFileSync(at('.filing/ledger.jsonl'), ''));
  const readme = at('.filing/README.md'); const rm = readText(readme);
  if (rm == null || rm.includes('generated by filing init')) plan('.filing/README.md', rm == null ? 'create' : 'refresh', () => writeIfChanged(readme, tpl('filing-README.md', { ...L, version: VERSION })));
  if (!exists(at(L.current))) plan(L.current, 'create', () => writeAtomic(at(L.current), tpl('STATUS.md', { date: today(), owner: cfg.owner || '(owner)', current: o.current || '(owner: name the one current thing)' })));
  if (!exists(at(`${L.archive}/INDEX.tsv`))) plan(`${L.archive}/INDEX.tsv`, 'create', () => { fs.mkdirSync(at(L.archive), { recursive: true }); fs.writeFileSync(at(`${L.archive}/INDEX.tsv`), 'archived\tfrom\tto\tsuperseded_by\twho\tsha256\n'); });
  // entry point: the card is rendered into the managed block of AGENTS.md (render fills it from evidence)
  const stub = `${BLOCK_BEGIN} v2 -->\n${BLOCK_END}`;
  const ag = readText(at('AGENTS.md'));
  if (ag == null) plan('AGENTS.md', 'create (facts card)', () => writeAtomic(at('AGENTS.md'), `${stub}\n\n## Commands\n\n<!-- Verified commands only: run, test, deploy. Record one only after it worked here. Secret NAMES, never values. -->\n`));
  else if (!ag.includes(BLOCK_BEGIN)) plan('AGENTS.md', 'add facts card at top (your text kept byte for byte)', () => writeAtomic(at('AGENTS.md'), upsertBlock(ag, stub)));
  else actions.push({ rel: 'AGENTS.md', what: 'card refreshed by render' });
  const imp = (file, line, why) => {
    const t = readText(at(file));
    if (t == null) plan(file, 'create (imports AGENTS.md)', () => writeAtomic(at(file), `${line}\n`));
    else if (!/^@\.?\/?AGENTS\.md\s*$/m.test(t)) plan(file, `append import (${why})`, () => fs.appendFileSync(at(file), `${t.endsWith('\n') || !t ? '' : '\n'}\n${line}\n`));
  };
  imp('CLAUDE.md', '@AGENTS.md', 'Claude Code reads CLAUDE.md');
  let gem = null; try { gem = JSON.parse(readText(at('.gemini/settings.json'))); } catch { /* none */ }
  if (gem && [].concat((gem.context && gem.context.fileName) || []).includes('AGENTS.md')) actions.push({ rel: 'GEMINI.md', what: 'skip (.gemini/settings.json already loads AGENTS.md)' });
  else imp('GEMINI.md', '@./AGENTS.md', 'Gemini CLI reads GEMINI.md by default');
  for (const p of cfg.packages) {
    const up = P.relative(p, '.') || '.';
    const pb = tpl('package-agents-block.md', { root: up, pkg: p, script: `${up}/.filing/filing.mjs` });
    const f = `${p}/AGENTS.md`; const t = readText(at(f));
    if (t == null) plan(f, 'create (package block)', () => writeAtomic(at(f), `${pb.trimEnd()}\n`));
    else if (upsertBlock(t, pb) !== t) plan(f, 'add/update package block', () => writeAtomic(at(f), upsertBlock(t, pb)));
  }
  // git plumbing
  const lines = (file, want) => { const t = readText(at(file)) || ''; const have = new Set(t.split(/\r?\n/).map((x) => x.trim())); const miss = want.filter((w) => !have.has(w)); if (miss.length) plan(file, `append ${miss.length} line(s)`, () => fs.appendFileSync(at(file), `${t && !t.endsWith('\n') ? '\n' : ''}${miss.join('\n')}\n`)); };
  lines('.gitattributes', [`.filing/ledger.jsonl merge=union eol=lf`, `${L.index} merge=union eol=lf`]);
  lines('.gitignore', ['.filing/lease.json*', '.filing/lane.json', '.filing/journal/', '.filing/trash/', '.filing/manifest.tsv', '*.tmp-*']);
  const report = actions.map((a) => `${a.what.padEnd(28)} ${a.rel}`);
  if (o.dryRun) return { mode, dryRun: true, report, git: gitRepo, packages: cfg.packages };
  for (const a of actions) if (a.fn) a.fn();
  const changed = withLease(root, 'init', () => renderAll(context(root)));
  return { mode, report, rendered: changed, git: gitRepo, packages: cfg.packages };
}

// ---------------------------------------------------------------- CLI
const HELP = `filing.mjs ${VERSION} — node .filing/filing.mjs <command>
  init [dir] [--mode new|adopt] [--preset numbered] [--current "<text>"] [--owner X] [--dry-run]
  status                                   live: current, git branch/HEAD/dirty/ahead-behind, items, next
  item add "<title>" [--id X] [--home path] [--pkg dir] [--files a,b]
  item start|done|reopen <id> | item drop <id> --why "<reason>" | item note <id> "<text>" | item set <id> --home ...
  lesson add "<what went wrong>" --do "<what to do instead>" | lesson retire <id> --why "..." | lesson list
  check [--strict] [--json] [--pkg dir] [--cold]   exit 1 on FAIL (and on WARN with --strict); --cold prints the cold-start table
  render                                   rebuild the AGENTS.md card and INDEX.md from evidence (after any change, merge or rebase)
  facts [dir] [--json]                     read-only: schedules (cron, systemd, Task Scheduler XML), config, outputs, runbooks
  close "<summary>" --next "<exact command>" [--verified "claim — how"]... [--unverified "..."]... [--blocker "..."]... [--no-fetch]
  archive <file> --by "<replacement>"      supersede a doc: moved byte for byte, hash indexed, links repointed
  survey [dir] [--out plan.json] [--keep path]...   read-only; writes plan.json + plan.md only with --out
  apply <plan.json> [--yes]                dry run without --yes; journaled; rewrites markdown links
  undo [journal-dir]                       reverses the last apply exactly, or refuses
  refs [--main branch]                     branches merged by content (squash/rebase aware), worktrees
  lane [show|claim <why>|release] [--force]
Set FILING_WHO=<name> so leases, lanes and the ledger say who acted.`;

export function parseArgs(argv) {
  const pos = []; const o = {}; const multi = new Set(['verified', 'unverified', 'blocker', 'keep']);
  const BOOL = new Set(['yes', 'strict', 'json', 'dry-run', 'force', 'no-fetch', 'help', 'cold']);
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith('--')) {
      const [k0, v0] = a.slice(2).split(/=(.*)/s); const k = k0.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
      const val = v0 !== undefined ? v0 : BOOL.has(k0) ? true : (argv[i + 1] !== undefined && !argv[i + 1].startsWith('--') ? argv[++i] : true);
      if (multi.has(k0)) { const key = k0 === 'blocker' ? 'blockers' : k; (o[key] ||= []).push(val); } else o[k] = val;
    } else pos.push(a);
  }
  return { pos, o };
}
const printFindings = (r, strict) => {
  for (const [lvl, list] of [['FAIL', r.fail], ['WARN', r.warn], ['INFO', r.info]]) for (const x of list) console.log(`${lvl}  ${x.code.padEnd(15)} ${x.path ? `${x.path}: ` : ''}${x.msg}`);
  const red = r.fail.length || (strict && r.warn.length);
  console.log(red ? `check: RED — ${r.fail.length} fail, ${r.warn.length} warn` : `check: green — ${r.warn.length} warn${r.warn.length ? ' (run with --strict to make warnings fail)' : ''}`);
  return red ? 1 : 0;
};
export function main(argv) {
  const { pos, o } = parseArgs(argv); const cmd = pos.shift();
  if (!cmd || cmd === 'help' || o.help) { console.log(HELP); return 0; }
  if (cmd === 'init') { const r = init(pos[0] || process.cwd(), o); console.log(`${r.dryRun ? 'would write' : 'init'} (${r.mode}${r.git ? ', git' : ', no git'}${r.packages.length ? `, packages: ${r.packages.join(' ')}` : ''}):\n  ${r.report.join('\n  ')}`); if (!r.dryRun) { console.log(`next: write "Current:" and the "## In progress" lines in ${loadConfig(path.resolve(pos[0] || process.cwd())).layout.current} (the owner's words), then: node .filing/filing.mjs render && node .filing/filing.mjs check --cold`); } return 0; }
  const root = findRoot(o.root || (['survey', 'facts'].includes(cmd) ? pos[0] : undefined));
  switch (cmd) {
    case 'status': case 'handoff': console.log(status(root)); return 0;
    case 'facts': { const ctx = context(root); const F = facts(ctx); console.log(o.json ? JSON.stringify(F, null, 2) : factsMarkdown(F)); return 0; }
    case 'check': { const r = check(root, o); if (o.json) { console.log(JSON.stringify(r, null, 2)); return r.fail.length || (o.strict && r.warn.length) ? 1 : 0; } return printFindings(r, o.strict); }
    case 'render': { const ch = withLease(root, 'render', () => renderAll(context(root))); console.log(ch.length ? `rendered: ${ch.join(', ')}` : 'generated docs already current'); return 0; }
    case 'item': { const r = item(root, pos[0], pos.slice(1), o); console.log(`${pos[0]} ${r.id}: wrote ${r.changed.join(', ')}`); return 0; }
    case 'close': { const rel = close(root, pos.join(' '), o); console.log(`session log: ${rel}`); return printFindings(check(root), false); }
    case 'archive': { const r = archive(root, pos[0], o.by); console.log(`archived -> ${r.dest}`); if (r.relinked.length) console.log(`links repointed in: ${r.relinked.join(', ')}`); if (r.report.length) console.log(`still link to the old doc (fix by hand): ${r.report.join(', ')}`); return 0; }
    case 'survey': {
      const r = survey(root, o);
      if (o.out) { const out = path.resolve(o.out); writeAtomic(out, `${JSON.stringify(r, null, 2)}\n`); writeAtomic(out.replace(/\.json$/i, '') + '.md', planMarkdown(r)); console.log(`plan: ${o.out} (+ .md)`); }
      const s = r.summary; console.log(`survey: ${r.files} files; ${s.move} proposed moves; ${s.pinned} pinned; ${s.decisions} owner decisions; ${s.duplicates} duplicate groups; ${s.secrets} secret-named; ${s.hazards} Windows hazards; ${s.broken} broken links; ${s.stale} stale refs.${o.out ? '' : ' Read-only: pass --out <file> to write a plan.'}`);
      return 0;
    }
    case 'apply': {
      if (!pos[0]) throw new Fail('usage: apply <plan.json> [--yes]', 2);
      const plan = JSON.parse(fs.readFileSync(path.resolve(pos[0]), 'utf8'));
      const r = apply(root, plan, o);
      if (r.dryRun) { console.log(`DRY RUN (nothing changed). Would move ${r.moves.length} file(s) and rewrite links in ${r.edits.length}:`); for (const m of r.moves) console.log(`  ${m.from}  ->  ${m.to}`); console.log('Re-run with --yes to apply.'); return 0; }
      console.log(`moved ${r.moved}; links rewritten in ${r.edited.length} doc(s); journal ${r.journal}\nundo: node .filing/filing.mjs undo`);
      if (r.unedited.length) console.log(`NOT edited (check by hand):\n  ${r.unedited.join('\n  ')}`);
      return 0;
    }
    case 'undo': { const r = undo(root, pos[0]); console.log(`undone: ${r.restored} move(s), ${r.edits} link edit(s) restored${r.leftovers.length ? `; could not remove ${r.leftovers.join(', ')}` : ''}`); return 0; }
    case 'refs': { const r = refs(root, o.main); console.log(`main: ${r.main}\nmerged into main (safe to delete after review):`); r.merged.forEach((b) => console.log(`  ${b.name} ${b.sha} — ${b.how}`)); console.log('NOT merged (work to merge, or a decision to drop):'); r.unmerged.forEach((b) => console.log(`  ${b.name} ${b.sha}`)); r.unknown.forEach((b) => console.log(`  ${b.name} — ${b.why}`)); console.log('worktrees:'); r.worktrees.forEach((w) => console.log(`  ${w.path} ${w.branch || ''}`)); return 0; }
    case 'lesson': { const r = (pos[0] === 'list') ? lessonLocked(root, 'list', [], o) : withLease(root, 'lesson', () => lessonLocked(root, pos[0], pos.slice(1), o)); console.log(r.text); return 0; }
    case 'lane': console.log(lane(root, pos[0], pos.slice(1), o)); return 0;
    default: console.error(`unknown command "${cmd}"\n${HELP}`); return 2;
  }
}
if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try { process.exitCode = main(process.argv.slice(2)); } catch (e) { if (e instanceof Fail) { console.error(e.message); process.exitCode = e.exit; } else { console.error(e.stack || e); process.exitCode = 1; } }
}
