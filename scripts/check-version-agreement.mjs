// Every manifest that carries the app's version must carry the same one.
// Usage: node scripts/check-version-agreement.mjs <cargo workspace version>
import { readFileSync } from "node:fs";

const expected = process.argv[2];
if (!expected) {
  console.error("usage: check-version-agreement.mjs <version>");
  process.exit(2);
}

const sources = {
  "package.json": JSON.parse(readFileSync("package.json", "utf8")).version,
  "src-tauri/tauri.conf.json": JSON.parse(
    readFileSync("src-tauri/tauri.conf.json", "utf8"),
  ).version,
};

let failed = false;
for (const [file, version] of Object.entries(sources)) {
  if (version !== expected) {
    console.error(`${file} says ${version}; Cargo.toml says ${expected}`);
    failed = true;
  }
}
if (failed) process.exit(1);
console.log(`version agreement OK (${expected})`);
