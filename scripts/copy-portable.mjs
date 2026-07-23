import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const localAppData =
  process.env.LOCALAPPDATA || join(homedir(), "AppData", "Local");
const cargoTarget =
  process.env.CARGO_TARGET_DIR || join(localAppData, "headroom-cargo-target");

const candidates = [
  join(cargoTarget, "release", "headroom.exe"),
  join(root, "src-tauri", "target", "release", "headroom.exe"),
];

const src = candidates.find((p) => existsSync(p));
if (!src) {
  console.error(
    `Missing release binary. Looked in:\n${candidates.map((p) => `  ${p}`).join("\n")}\nRun: npm run build:portable`,
  );
  process.exit(1);
}

const outDir = join(root, "portable");
const dest = join(outDir, "HeadRoom.exe");
mkdirSync(outDir, { recursive: true });
copyFileSync(src, dest);
console.log(`Portable binary ready:\n  ${dest}`);
console.log(
  "Requires WebView2 (included with Windows 11). Copy this single .exe anywhere and run it.",
);
