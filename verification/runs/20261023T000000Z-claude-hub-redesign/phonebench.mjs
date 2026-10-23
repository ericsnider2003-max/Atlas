// Low-end phone over Tailscale: 360x740, 4x CPU slowdown, 150 ms RTT, 1.6 Mbps down.
// usage: node phonebench.mjs <base> <token> <outdir> [label] [paths...]
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync, mkdtempSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const [base, token, out, label = "before", ...paths] = process.argv.slice(2);
mkdirSync(out, { recursive: true });
const edge = "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe";
const port = 9300 + Math.floor(Math.random() * 500);
const prof = mkdtempSync(join(tmpdir(), "pb-"));
const proc = spawn(edge, ["--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${prof}`, "--no-first-run", "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let tgt;
for (let i = 0; i < 40 && !tgt; i++) { await sleep(250); try { tgt = (await (await fetch(`http://127.0.0.1:${port}/json`)).json()).find((t) => t.type === "page"); } catch {} }
const ws = new WebSocket(tgt.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0; const pending = new Map(); const listeners = [];
ws.addEventListener("message", (e) => { const m = JSON.parse(e.data); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } else listeners.forEach((f) => f(m)); });
const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const waitEvent = (name, ms = 30000) => new Promise((r) => { const t = setTimeout(() => r(null), ms); listeners.push((m) => { if (m.method === name) { clearTimeout(t); r(m); } }); });

await send("Page.enable"); await send("Network.enable"); await send("Runtime.enable");
await send("Emulation.setDeviceMetricsOverride", { width: 360, height: 740, deviceScaleFactor: 2, mobile: true });
await send("Emulation.setUserAgentOverride", { userAgent: "Mozilla/5.0 (Linux; Android 11; Low-end) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120 Mobile Safari/537.36" });
// sign in once, unthrottled
let ld = waitEvent("Page.loadEventFired"); await send("Page.navigate", { url: `${base}/hub?t=${token}` }); await ld;
await send("Network.setBypassServiceWorker", { bypass: true });
await send("Network.emulateNetworkConditions", { offline: false, latency: 150, downloadThroughput: 200000, uploadThroughput: 90000 });
await send("Emulation.setCPUThrottlingRate", { rate: 4 });
const rows = [];
for (const p of paths) {
  const url = `${base}${p}`;
  let bytes = 0; const onData = (m) => { if (m.method === "Network.loadingFinished") bytes += m.params.encodedDataLength; };
  listeners.push(onData);
  ld = waitEvent("Page.loadEventFired");
  await send("Page.navigate", { url }); await ld; await sleep(300);
  listeners.splice(listeners.indexOf(onData), 1);
  const r = await send("Runtime.evaluate", { returnByValue: true, expression: `(()=>{const n=performance.getEntriesByType('navigation')[0];const fcp=performance.getEntriesByName('first-contentful-paint')[0];return {ttfb:Math.round(n.responseStart),fcp:fcp?Math.round(fcp.startTime):null,load:Math.round(n.loadEventEnd||n.duration),dom:document.getElementsByTagName('*').length,h:document.documentElement.scrollHeight}})()` });
  const v = r.result.result.value; rows.push({ path: p, bytes, ...v });
  const shot = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(out, `${label}-${p.replace(/[^a-z0-9]+/gi, "_") || "home"}.png`), Buffer.from(shot.result.data, "base64"));
}
console.log(JSON.stringify(rows));
writeFileSync(join(out, `${label}.json`), JSON.stringify(rows, null, 1));
ws.close(); proc.kill();
