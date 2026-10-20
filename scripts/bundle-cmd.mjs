// beforeBundleCommand: builds wattclean-cmd for the target being bundled and
// puts it where bundle.externalBin looks, src-tauri/binaries/
// wattclean-cmd-<triple>, so every bundle (NSIS, app, deb, rpm, AppImage)
// installs it beside the app, and a signed macOS build would sign it too.
// Tauri names the target in TAURI_ENV_TARGET_TRIPLE; a universal macOS build
// gets both architectures joined by lipo. Cargo reports its own target folder,
// so CARGO_TARGET_DIR is honoured.
import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const triple = process.env.TAURI_ENV_TARGET_TRIPLE;
if (!triple) {
  console.error(
    "TAURI_ENV_TARGET_TRIPLE is not set: run this through `tauri build`.",
  );
  process.exit(2);
}

const run = (program, args) =>
  execFileSync(program, args, { cwd: root, stdio: "inherit" });

const exe = triple.includes("windows") ? ".exe" : "";
const triples =
  triple === "universal-apple-darwin"
    ? ["aarch64-apple-darwin", "x86_64-apple-darwin"]
    : [triple];

const { target_directory } = JSON.parse(
  execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
    cwd: root,
    encoding: "utf8",
  }),
);
run("cargo", [
  "build",
  "-p",
  "wattclean-cmd",
  "--release",
  "--locked",
  ...triples.flatMap((t) => ["--target", t]),
]);
const built = triples.map((t) =>
  join(target_directory, t, "release", `wattclean-cmd${exe}`),
);

const dest = join(
  root,
  "src-tauri",
  "binaries",
  `wattclean-cmd-${triple}${exe}`,
);
mkdirSync(dirname(dest), { recursive: true });
if (built.length > 1) run("lipo", ["-create", "-output", dest, ...built]);
else copyFileSync(built[0], dest);
if (!exe) chmodSync(dest, 0o755);
console.log(`wattclean-cmd for ${triple}: ${dest}`);
