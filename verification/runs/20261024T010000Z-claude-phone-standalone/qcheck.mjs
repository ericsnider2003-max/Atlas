// Quality check of a GGUF model, offline, through llama-completion.
// usage: node qcheck.mjs <model.gguf> <label>
import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const [model, label] = process.argv.slice(2);
const exe = "C:/Users/erics/AppData/Local/Atlas/tools/llama/llama-completion.exe";
const NL = String.fromCharCode(10);
// [question, any-of expected substrings (lower case)]
const cases = [
  ["What is 17 times 3? Answer with the number only.", ["51"]],
  ["What is the capital of Australia? One word.", ["canberra"]],
  ["Turn this into a short reminder title: call the dentist about the crown on tuesday", ["dentist"]],
  ["If it is 14:40 now, what time is it in 35 minutes? Answer as HH:MM.", ["15:15"]],
  ["Which is bigger, 0.7 or 0.65? Answer with the number only.", ["0.7"]],
  ["Spell the word 'necessary' backwards.", ["yrassecen"]],
  ["Summarise in five words or fewer: The meeting moved from Monday to Wednesday at 3pm because Sam is away.", ["wednesday"]],
  ["Is a tomato a fruit or a vegetable botanically? One word.", ["fruit"]],
  ["Translate 'good morning' into French.", ["bonjour"]],
  ["Name the planet closest to the Sun. One word.", ["mercury"]],
  ["Reply with exactly the word READY and nothing else.", ["ready"]],
  ["How many days are in a leap year? Number only.", ["366"]],
];
let right = 0, totalMs = 0;
const rows = [];
for (const [q, want] of cases) {
  const prompt = "<|im_start|>user" + NL + q + " /no_think<|im_end|>" + NL + "<|im_start|>assistant" + NL + "<think>" + NL + NL + "</think>" + NL + NL;
  const f = join(tmpdir(), "qcheck-prompt.txt");
  writeFileSync(f, prompt);
  const t = Date.now();
  const r = spawnSync(exe, ["-m", model, "-f", f, "-n", "48", "--temp", "0", "-no-cnv", "--offline", "-ngl", "0", "-t", "4", "--no-warmup", "--no-display-prompt", "-r", "<|im_end|>"], { encoding: "utf8", timeout: 120000 });
  const ms = Date.now() - t; totalMs += ms;
  const out = (r.stdout || "").replace(/<\|im_end\|>/g, "").trim().split(NL)[0].slice(0, 80);
  const ok = want.some((w) => out.toLowerCase().includes(w));
  right += ok;
  rows.push(`${ok ? "PASS" : "MISS"} ${String(ms).padStart(5)}ms  ${q.slice(0, 50)} -> ${out}`);
}
console.log(rows.join(NL));
console.log(`QCHECK ${label}: ${right}/${cases.length} right, mean ${Math.round(totalMs / cases.length)} ms per answer (CPU, 4 threads, offline)`);
