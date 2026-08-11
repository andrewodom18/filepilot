import { readFile } from "node:fs/promises";
const root = new URL("../", import.meta.url);
const readJson = async (relativePath) => JSON.parse(await readFile(new URL(relativePath, root), "utf8"));

const [cargoToml, packageJson, packageLock, tauriConfig] = await Promise.all([
  readFile(new URL("Cargo.toml", root), "utf8"),
  readJson("apps/filepilot-desktop/package.json"),
  readJson("apps/filepilot-desktop/package-lock.json"),
  readJson("apps/filepilot-desktop/src-tauri/tauri.conf.json"),
]);

const workspaceVersion = cargoToml.match(/\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
const versions = new Map([
  ["Cargo workspace", workspaceVersion],
  ["desktop package", packageJson.version],
  ["desktop lockfile", packageLock.packages?.[""]?.version],
  ["Tauri bundle", tauriConfig.version],
]);
const expected = versions.get("Cargo workspace");
const mismatches = [...versions].filter(([, version]) => !version || version !== expected);

if (!expected || mismatches.length > 0) {
  for (const [name, version] of versions) console.error(`${name}: ${version ?? "missing"}`);
  throw new Error("FilePilot version metadata is inconsistent");
}

console.log(`FilePilot version metadata is consistent: ${expected}`);
