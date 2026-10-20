// The operating system the app runs on: which modules the window offers
// (the registry exists on Windows only) and the words it uses for the
// system's own things. Set once at start, from the app's own answer.

import type { Platform } from "./api.ts";

let current: Platform = "windows";

export const setPlatform = (platform: Platform): void => {
  current = platform;
};

export const platform = (): Platform => current;

export const isWindows = (): boolean => current === "windows";

/** The system's name, as people say it. */
export const osName = (): string =>
  current === "macos" ? "macOS" : current === "linux" ? "Linux" : "Windows";

/** Where deleted files wait: the Recycle Bin or the Trash. */
export const trashName = (): string =>
  current === "windows" ? "Recycle Bin" : "Trash";

/** Where the app's icon waits while the window is closed. */
export const trayName = (): string =>
  current === "windows"
    ? "notification area"
    : current === "macos"
      ? "menu bar"
      : "system tray";

/** How a drive's own encryption is called, for advice on SSDs. */
export const diskEncryption = (): string =>
  current === "macos"
    ? "FileVault"
    : current === "linux"
      ? "full-disk encryption (LUKS)"
      : "Windows' own BitLocker encryption";

/** How a program is ended: macOS says "quit", the others "close". */
const quit = (): string => (current === "macos" ? "quit" : "close");

/** The clean's confirmation line for a ticked program that is running.
 *  Windows will not delete a file in use, so only those are skipped; macOS
 *  and Linux would delete them from under it, so its options are left
 *  alone until it is closed. */
export const runningWarning = (name: string): string =>
  current === "windows"
    ? `${name} is running: files it has open will be skipped.`
    : `${name} is running, so it will be left alone: ${quit()} it first to clean it.`;

/** The report's banner for the programs that were running. */
export const runningBanner = (names: string[]): string => {
  const one = names.length === 1;
  const list = names.join(", ");
  return current === "windows"
    ? `Running now: ${list}. Close ${one ? "it" : "them"} before cleaning, or files ${one ? "it has" : "they have"} open are skipped.`
    : `Running now: ${list}. ${one ? "It was" : "They were"} left alone: ${quit()} ${one ? "it" : "them"}, then clean again.`;
};

/** What else a free-space wipe of this drive costs: a full start-up disk
 *  makes macOS purge local snapshots and downloaded iCloud copies, and the
 *  built-in SSD of a recent Mac is encrypted anyway. Other drives (offered
 *  as removable) hold neither. */
export const wipeCaveat = (removable: boolean): string =>
  current === "macos" && !removable
    ? "While the disk is full, macOS makes room by removing Time Machine's local snapshots, and may remove downloaded copies of iCloud files, so files deleted earlier can no longer be restored from a local snapshot. On an encrypted SSD, as every Mac with Apple silicon or a T2 chip has built in, the wipe adds little."
    : "";

/** Whether two paths name one file: Windows and macOS (on its default
 *  disks) ignore case in names, Linux does not. */
export const samePath = (a: string, b: string): boolean =>
  current === "linux" ? a === b : a.toLowerCase() === b.toLowerCase();
