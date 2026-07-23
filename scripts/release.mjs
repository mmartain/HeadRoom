import { spawnSync } from "node:child_process";
import { bumpVersion, getVersion, setVersion } from "./bump-version.mjs";

function run(cmd, args, opts = {}) {
  const result = spawnSync(cmd, args, {
    stdio: "inherit",
    // Keep git args intact on Windows (no cmd.exe re-parsing).
    shell: false,
    ...opts,
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function runCapture(cmd, args) {
  const result = spawnSync(cmd, args, {
    encoding: "utf8",
    shell: false,
  });
  if (result.status !== 0) {
    throw new Error(result.stderr || `Command failed: ${cmd} ${args.join(" ")}`);
  }
  return (result.stdout || "").trim();
}

const kind = process.argv[2];
if (!kind) {
  console.error(`Usage:
  npm run release -- patch
  npm run release -- minor
  npm run release -- major
  npm run release -- 0.2.0

This bumps package.json / Cargo.toml / tauri.conf.json (when needed), commits,
tags vX.Y.Z, and pushes. GitHub Actions then builds HeadRoom.exe (+ NSIS) and
publishes a GitHub Release.`);
  process.exit(1);
}

const status = runCapture("git", ["status", "--porcelain"]);
if (status) {
  console.error("Working tree is not clean. Commit or stash first.");
  process.exit(1);
}

const branch = runCapture("git", ["rev-parse", "--abbrev-ref", "HEAD"]);
if (branch !== "main") {
  console.error(`Refuse to release from branch '${branch}' (expected main).`);
  process.exit(1);
}

const current = getVersion();
const next = bumpVersion(current, kind);
const tag = `v${next}`;

const existing = spawnSync(
  "git",
  ["rev-parse", "-q", "--verify", `refs/tags/${tag}`],
  {
    encoding: "utf8",
    shell: false,
  },
);
if (existing.status === 0) {
  console.error(`Tag ${tag} already exists.`);
  process.exit(1);
}

if (next !== current) {
  console.log(`Releasing ${current} → ${next}`);
  setVersion(next);
  run("git", [
    "add",
    "package.json",
    "package-lock.json",
    "src-tauri/Cargo.toml",
    "src-tauri/tauri.conf.json",
  ]);
  run("git", ["add", "-u", "src-tauri/Cargo.lock"]);
  run("git", ["commit", "-m", `Release ${tag}`]);
} else {
  console.log(`Version already ${next}; creating tag ${tag} only.`);
}

run("git", ["tag", "-a", tag, "-m", `HeadRoom ${tag}`]);
run("git", ["push", "origin", "HEAD"]);
run("git", ["push", "origin", tag]);

console.log(`
Pushed ${tag}.
GitHub Actions will build artifacts and create:
  https://github.com/mmartain/HeadRoom/releases/tag/${tag}
`);
