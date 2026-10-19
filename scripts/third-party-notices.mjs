// Writes THIRD-PARTY-NOTICES.txt: the licence of every open-source package built into
// wattclean and wattclean-cmd, as their licences ask when the programs are given out.
// Rust packages come from `cargo metadata` (normal dependencies of the two shipped crates, on
// every platform WattClean ships for: one file serves the Windows, Linux and macOS builds);
// the page's one npm runtime package from node_modules. Identical licence texts are printed
// once with the packages that share them.
//
// Usage: node scripts/third-party-notices.mjs [--check]
//   --check  exits 1 when the committed file differs from what would be written.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = path.join(root, "THIRD-PARTY-NOTICES.txt");
const SHIPPED = ["wattclean", "wattclean-cmd"];
const NPM_RUNTIME = ["@tauri-apps/api"];
const LICENCE_FILE = /^(licen[cs]e|copying|notice|copyright)/i;

/** The platforms releases are built for. */
const TARGETS = [
  "x86_64-pc-windows-msvc",
  "x86_64-unknown-linux-gnu",
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
];

// Normal (not build or dev) dependencies reachable from the shipped crates,
// on any of the targets.
const reached = new Set();
const packages = new Map();
for (const target of TARGETS) {
  const metadata = JSON.parse(
    execFileSync(
      "cargo",
      [
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--filter-platform",
        target,
      ],
      { cwd: root, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
    ),
  );
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
  for (const pkg of metadata.packages) packages.set(pkg.id, pkg);
  const stack = metadata.packages
    .filter((pkg) => SHIPPED.includes(pkg.name) && pkg.source === null)
    .map((pkg) => pkg.id);
  if (stack.length !== SHIPPED.length) {
    throw new Error(`expected the crates ${SHIPPED.join(", ")} in the workspace`);
  }
  const seen = new Set();
  while (stack.length > 0) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    reached.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      if (dep.dep_kinds.some((kind) => kind.kind === null)) stack.push(dep.pkg);
    }
  }
}

/** The licence files a package ships, as [name, text] pairs, sorted by name. */
const licenceFiles = (folder) =>
  existsSync(folder)
    ? readdirSync(folder, { withFileTypes: true })
        .filter((entry) => entry.isFile() && LICENCE_FILE.test(entry.name))
        .map((entry) => entry.name)
        .sort()
        .map((name) => [
          name,
          readFileSync(path.join(folder, name), "utf8")
            .replace(/\r\n/g, "\n")
            .trim(),
        ])
    : [];

const MIT = `Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in the
Software without restriction, including without limitation the rights to use, copy,
modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and
to permit persons to whom the Software is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice shall be included in all copies
or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR
THE USE OR OTHER DEALINGS IN THE SOFTWARE.`;

const BSD3 = `Redistribution and use in source and binary forms, with or without modification, are
permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of
   conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list
   of conditions and the following disclaimer in the documentation and/or other
   materials provided with the distribution.
3. Neither the name of the copyright holder nor the names of its contributors may be
   used to endorse or promote products derived from this software without specific
   prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY
EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL
THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT
OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.`;

/** For a package that ships no licence file: the standard text of its stated licence. */
const standardText = (pkg) => {
  const holders =
    pkg.authors.length > 0 ? pkg.authors.join(", ") : `the ${pkg.name} authors`;
  const source = pkg.repository ?? pkg.homepage ?? "its crates.io page";
  const intro = `${pkg.name} ships no licence file; this is the standard text of its stated licence (${pkg.license}).`;
  if (/\bMIT\b/.test(pkg.license ?? "")) {
    return `${intro}\n\nCopyright (c) ${holders}\n\n${MIT}`;
  }
  if (/BSD-3-Clause/.test(pkg.license ?? "")) {
    return `${intro}\n\nCopyright (c) ${holders}\n\n${BSD3}`;
  }
  if (/MPL-2\.0/.test(pkg.license ?? "")) {
    return `${intro}\n\nThis package is subject to the Mozilla Public License, v. 2.0 (full text above under\nanother package, and at https://mozilla.org/MPL/2.0/). Its source code is available\nfrom ${source}.`;
  }
  return null;
};

const entries = [];
for (const id of reached) {
  const pkg = packages.get(id);
  // Workspace crates are WattClean's own (MIT, see LICENSE).
  if (!pkg || pkg.source === null) continue;
  let files = licenceFiles(path.dirname(pkg.manifest_path));
  const fallback = files.length === 0 ? standardText(pkg) : null;
  if (fallback) files = [["(standard text)", fallback]];
  entries.push({
    label: `${pkg.name} ${pkg.version}`,
    licence:
      pkg.license ?? (pkg.license_file ? "see its licence file" : "unstated"),
    home: pkg.repository ?? pkg.homepage ?? "",
    files,
    standard: fallback !== null,
  });
}
for (const name of NPM_RUNTIME) {
  const folder = path.join(root, "node_modules", ...name.split("/"));
  const manifest = JSON.parse(
    readFileSync(path.join(folder, "package.json"), "utf8"),
  );
  const repo =
    typeof manifest.repository === "string"
      ? manifest.repository
      : (manifest.repository?.url ?? "");
  entries.push({
    label: `${manifest.name} ${manifest.version} (npm)`,
    licence: manifest.license ?? "unstated",
    home: repo,
    files: licenceFiles(folder),
  });
}
entries.sort((a, b) => a.label.localeCompare(b.label));

// Each distinct licence text once, with the packages that ship it.
const texts = new Map();
for (const entry of entries) {
  for (const [, text] of entry.files) {
    const key = createHash("sha256").update(text).digest("hex");
    const seen = texts.get(key) ?? { text, users: [] };
    seen.users.push(entry.label);
    texts.set(key, seen);
  }
}

const lines = [
  "Third-party notices for WattClean",
  "",
  "WattClean (wattclean.exe and wattclean-cmd.exe) is MIT-licensed; see LICENSE. It",
  "includes the open-source packages below, each under the licence shown. Generated by",
  "scripts/third-party-notices.mjs from the locked dependency list; do not edit by hand.",
  "",
  `Packages (${entries.length}):`,
  "",
  ...entries.map(
    (entry) =>
      `  ${entry.label} - ${entry.licence}${entry.home ? ` - ${entry.home}` : ""}${
        entry.files.length === 0
          ? " (ships no licence file)"
          : entry.standard
            ? " (standard licence text)"
            : ""
      }`,
  ),
  "",
  "Licence texts:",
];
for (const { text, users } of [...texts.values()].sort((a, b) =>
  a.users[0].localeCompare(b.users[0]),
)) {
  lines.push(
    "",
    "=".repeat(78),
    `Used by: ${users.join(", ")}`,
    "-".repeat(78),
    text,
  );
}
const content = `${lines.join("\n")}\n`;

if (process.argv.includes("--check")) {
  const current = existsSync(output)
    ? readFileSync(output, "utf8").replace(/\r\n/g, "\n")
    : "";
  if (current !== content) {
    console.error(
      "THIRD-PARTY-NOTICES.txt is out of date: run node scripts/third-party-notices.mjs",
    );
    process.exit(1);
  }
  console.log(`third-party notices OK (${entries.length} packages)`);
} else {
  writeFileSync(output, content);
  console.log(
    `wrote ${path.relative(root, output)}: ${entries.length} packages, ${texts.size} licence texts`,
  );
}
