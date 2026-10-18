/**
 * The uninstaller's own folder inside the suite's data folder, so its specs
 * read and write their files (settings, backups, the test machine) there.
 */
import fs from "node:fs";
import path from "node:path";

import { dataDir as root } from "../../workspace.ts";

export { appPids, stopApp } from "../../workspace.ts";

export function dataDir(): string {
  return path.join(root(), "uninstall");
}

/** A path inside the test machine, e.g. `Program Files\Acme\Widget`. */
export function machinePath(relative: string): string {
  return path.join(dataDir(), "machine", relative);
}

export function readJson(name: string): unknown {
  const file = path.join(dataDir(), name);
  return fs.existsSync(file)
    ? JSON.parse(fs.readFileSync(file, "utf8"))
    : undefined;
}
