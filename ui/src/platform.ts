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

/** Whether two paths name one file: Windows and macOS (on its default
 *  disks) ignore case in names, Linux does not. */
export const samePath = (a: string, b: string): boolean =>
  current === "linux" ? a === b : a.toLowerCase() === b.toLowerCase();
