import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { pathToFileURL } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function writeJson(path, value) {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

export function getVersion() {
  return readJson(join(root, "package.json")).version;
}

export function parseSemver(version) {
  const m = /^(\d+)\.(\d+)\.(\d+)(?:-(.+))?$/.exec(version);
  if (!m) throw new Error(`Invalid semver: ${version}`);
  return {
    major: Number(m[1]),
    minor: Number(m[2]),
    patch: Number(m[3]),
    prerelease: m[4] ?? null,
  };
}

export function bumpVersion(current, kind) {
  if (/^\d+\.\d+\.\d+/.test(kind)) return kind.replace(/^v/, "");
  const v = parseSemver(current);
  if (kind === "major") return `${v.major + 1}.0.0`;
  if (kind === "minor") return `${v.major}.${v.minor + 1}.0`;
  if (kind === "patch") return `${v.major}.${v.minor}.${v.patch + 1}`;
  throw new Error(`Unknown bump kind: ${kind} (use patch|minor|major|x.y.z)`);
}

export function setVersion(version) {
  const normalized = version.replace(/^v/, "");
  parseSemver(normalized);

  const pkgPath = join(root, "package.json");
  const pkg = readJson(pkgPath);
  pkg.version = normalized;
  writeJson(pkgPath, pkg);

  const lockPath = join(root, "package-lock.json");
  const lock = readJson(lockPath);
  lock.version = normalized;
  if (lock.packages?.[""]) lock.packages[""].version = normalized;
  writeJson(lockPath, lock);

  const tauriPath = join(root, "src-tauri", "tauri.conf.json");
  const tauri = readJson(tauriPath);
  tauri.version = normalized;
  writeJson(tauriPath, tauri);

  const cargoPath = join(root, "src-tauri", "Cargo.toml");
  const cargo = readFileSync(cargoPath, "utf8");
  const nextCargo = cargo.replace(
    /^version\s*=\s*"[^"]+"/m,
    `version = "${normalized}"`,
  );
  if (nextCargo === cargo) {
    throw new Error("Could not update version in src-tauri/Cargo.toml");
  }
  writeFileSync(cargoPath, nextCargo);

  return normalized;
}

const isDirectRun = process.argv[1]
  ? import.meta.url === pathToFileURL(process.argv[1]).href
  : false;

if (isDirectRun) {
  const kind = process.argv[2];
  if (!kind) {
    console.log(getVersion());
    process.exit(0);
  }
  const next = bumpVersion(getVersion(), kind);
  console.log(setVersion(next));
}
