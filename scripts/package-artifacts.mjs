import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const localAppData =
  process.env.LOCALAPPDATA || join(homedir(), "AppData", "Local");
const cargoTarget =
  process.env.CARGO_TARGET_DIR || join(localAppData, "headroom-cargo-target");

const exeCandidates = [
  join(cargoTarget, "release", "headroom.exe"),
  join(root, "src-tauri", "target", "release", "headroom.exe"),
  join(cargoTarget, "x86_64-pc-windows-msvc", "release", "headroom.exe"),
  join(
    root,
    "src-tauri",
    "target",
    "x86_64-pc-windows-msvc",
    "release",
    "headroom.exe",
  ),
];

const exe = exeCandidates.find((p) => existsSync(p));
if (!exe) {
  console.error(
    `Missing release binary. Looked in:\n${exeCandidates.map((p) => `  ${p}`).join("\n")}`,
  );
  process.exit(1);
}

const outDir = join(root, "portable");
const dest = join(outDir, "HeadRoom.exe");
mkdirSync(outDir, { recursive: true });
copyFileSync(exe, dest);
console.log(`Portable binary ready:\n  ${dest}`);

const nsisDirs = [
  join(cargoTarget, "release", "bundle", "nsis"),
  join(root, "src-tauri", "target", "release", "bundle", "nsis"),
  join(cargoTarget, "x86_64-pc-windows-msvc", "release", "bundle", "nsis"),
];

for (const dir of nsisDirs) {
  if (!existsSync(dir)) continue;
  const setups = readdirSync(dir).filter((f) => f.toLowerCase().endsWith(".exe"));
  for (const name of setups) {
    const from = join(dir, name);
    const to = join(outDir, name);
    copyFileSync(from, to);
    console.log(`NSIS installer copied:\n  ${to}`);
  }
  // Copy updater signatures
  const sigs = readdirSync(dir).filter((f) => f.toLowerCase().endsWith(".exe.sig"));
  for (const name of sigs) {
    const from = join(dir, name);
    const to = join(outDir, name);
    copyFileSync(from, to);
    console.log(`Signature copied:\n  ${to}`);
  }
}

// Generate latest.json for the updater (NSIS only; portable uses custom download)
const { version } = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const setupSig = readdirSync(outDir)
  .find((f) => f.endsWith("x64-setup.exe.sig"));
if (setupSig) {
  const sig = readFileSync(join(outDir, setupSig), "utf8").trim();
  const setupName = setupSig.replace(/\.sig$/, "");
  const latest = {
    version,
    pub_date: new Date().toISOString(),
    platforms: {
      "windows-x86_64": {
        signature: sig,
        url: `https://github.com/mmartain/HeadRoom/releases/download/v${version}/${encodeURIComponent(setupName)}`,
      },
    },
  };
  const latestPath = join(outDir, "latest.json");
  writeFileSync(latestPath, JSON.stringify(latest, null, 2));
  console.log(`latest.json written:\n  ${latestPath}`);
}
