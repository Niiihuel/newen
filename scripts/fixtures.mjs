// Turns captures/<scenario>-<epoch>/ into anonymized fixtures:
// crates/quill-agent-claude/fixtures/<cli-version>/<scenario>.jsonl (docs/plan.md Q1.2).
// Usage: node scripts/fixtures.mjs captures/<dir>... [--name <fixture>]
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const args = process.argv.slice(2);
const nameAt = args.indexOf("--name");
const forced = nameAt >= 0 ? args.splice(nameAt, 2)[1] : null;
for (const dir of args) {
  const meta = JSON.parse(fs.readFileSync(path.join(dir, "meta.json"), "utf8"));
  const version = meta.cli_version.split(" ")[0];
  const repo = path.resolve(dir, "repo");
  const home = os.homedir();
  const lines = fs.readFileSync(path.join(dir, "session.jsonl"), "utf8").trim().split("\n");
  let emails = new Set();
  for (const l of lines) for (const m of l.matchAll(/[\w.+-]+@[\w-]+\.[\w.]+/g)) emails.add(m[0]);
  const clean = (s) => {
    // The CLI also keys its project directory by the path with every / as - ("-home-user-…").
    const dash = (p) => p.replaceAll("/", "-");
    let out = s.split(repo).join("/work").split(dash(repo)).join("-work");
    out = out.split(home).join("/home/user").split(dash(home)).join("-home-user");
    for (const e of emails) out = out.split(e).join("user@example.com");
    // What is left of the login: `ls -l` owners, and paths a stream cuts into pieces.
    out = out.replaceAll(os.userInfo().username, "user");
    // A Remote Control session's ids on claude.ai (`session_…`, `cse_…`) are the person's.
    out = out.replace(/\b(session|cse)_[0-9A-Za-z]{20,}/g, "$1_01REMOTE");
    return out;
  };
  const scrub = (v) => {
    if (typeof v === "string") return clean(v);
    if (Array.isArray(v)) return v.map(scrub);
    if (v && typeof v === "object") {
      const o = {};
      for (const [k, x] of Object.entries(v)) o[k] = k === "pid" && typeof x === "number" ? 0 : scrub(x);
      return o;
    }
    return v;
  };
  const out = lines.map((l) => JSON.stringify(scrub(JSON.parse(l))));
  const leak = out.find((l) => l.includes(home) || l.includes(home.replaceAll("/", "-")) || [...emails].some((e) => e !== "user@example.com" && l.includes(e)));
  if (leak) throw new Error(`personal data left in ${dir}: ${leak.slice(0, 120)}`);
  const name = forced ?? meta.scenario + (meta.args.includes("--fork-session") ? "-fork" : "");
  const target = path.join(path.dirname(new URL(import.meta.url).pathname), "../crates/quill-agent-claude/fixtures", version);
  fs.mkdirSync(target, { recursive: true });
  fs.writeFileSync(path.join(target, `${name}.jsonl`), out.join("\n") + "\n");
  console.log(`${dir} -> ${target}/${name}.jsonl (${out.length} lines)`);
}
