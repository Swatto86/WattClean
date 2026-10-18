/**
 * Isolated state for one acceptance run, and process lookups that never
 * match anything by executable name.
 *
 * The app reads `WATTCLEAN_DATA_DIR`; the driver launches it with this
 * process's environment, so pointing that at a fresh temp directory keeps
 * the suite away from an installed copy's settings and history, and puts
 * the test machine (its files and state.json) there too.
 */
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export const DATA_DIR_ENV = "WATTCLEAN_DATA_DIR";

export function prepareWorkspace(): string {
  const dir = fs.realpathSync.native(
    fs.mkdtempSync(path.join(os.tmpdir(), "wattclean-e2e-")),
  );
  process.env[DATA_DIR_ENV] = dir;
  return dir;
}

export function dataDir(): string {
  const dir = process.env[DATA_DIR_ENV];
  if (!dir) throw new Error(`${DATA_DIR_ENV} is not set`);
  return dir;
}

/** A path on the disk cleaner's test machine's drive C, e.g.
 * `Users\Tester\Documents`. */
export function machinePath(relative: string): string {
  return path.join(dataDir(), "disk", "machine", "C", relative);
}

/** What the test machine's Open dialogs answer next (see platform.rs). */
export function setPicks(picks: {
  files?: string[];
  folder?: string;
  ini?: string;
}): void {
  fs.writeFileSync(
    path.join(dataDir(), "disk", "picks.json"),
    JSON.stringify(picks),
  );
}

/** A JSON file in the data folder, e.g. `disk/history.json`. */
export function readJson(name: string): unknown {
  const file = path.join(dataDir(), name);
  return fs.existsSync(file)
    ? JSON.parse(fs.readFileSync(file, "utf8"))
    : undefined;
}

/** PIDs of running copies of exactly this executable. */
export function appPids(application: string): number[] {
  const script =
    "$p = $env:WC_E2E_APPLICATION; Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $p } | ForEach-Object { $_.ProcessId }";
  const result = execFileSync(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-Command", script],
    {
      encoding: "utf8",
      env: { ...process.env, WC_E2E_APPLICATION: application },
    },
  );
  return result.split(/\s+/).filter(Boolean).map(Number);
}

/**
 * End the copies of the e2e binary this suite launched: for a restart in
 * the middle of the suite, and whatever is left when the session ends. The
 * binary's path is the suite's own debug build, which onPrepare proved was
 * not running before.
 */
export async function stopApp(application: string): Promise<void> {
  for (const pid of appPids(application)) {
    execFileSync("taskkill", ["/PID", String(pid), "/F"], { stdio: "pipe" });
  }
  const deadline = Date.now() + 15_000;
  while (appPids(application).length > 0) {
    if (Date.now() > deadline) throw new Error("the e2e app did not stop");
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
}
