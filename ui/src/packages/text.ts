// What the uninstaller tells people about a removal on Linux and macOS,
// by where the program came from, and about what it left behind.

import { bytes } from "../format.ts";
import { platform, trashName } from "../platform.ts";
import type { LeftoverView } from "./api.ts";

/** How the removal goes, for the confirmation. */
export const removalHow = (source: string): string => {
  const macos = platform() === "macos";
  // Homebrew's sudo cannot ask in an app's missing terminal, so on macOS
  // WattClean gives it a password box (its removals run with SUDO_ASKPASS).
  const password =
    " If Homebrew needs an administrator's password, WattClean asks for it in a password box.";
  switch (source) {
    case "Application":
      return `Finder moves it to the ${trashName()}, asking for an administrator's password if it is not yours to move.`;
    case "AppImage":
      return `The file moves to the ${trashName()}.`;
    case "Homebrew":
      return macos
        ? `Homebrew removes it, first stopping its background service if it runs as one.${password}`
        : "Homebrew removes it.";
    case "Homebrew Cask":
      return macos
        ? `Homebrew Cask removes it.${password}`
        : "Homebrew Cask removes it.";
    default:
      return `${source} removes it, asking for an administrator's password when it needs one.`;
  }
};

/** What the button beside each leftover does. */
export const revealLabel = (): string =>
  platform() === "macos" ? "Show in Finder" : "Open the folder holding it";

/** What is said beside a leftover: what it is, and its size. */
export const leftoverNote = (leftover: LeftoverView): string =>
  [
    leftover.job ? "background job" : null,
    leftover.systemWide ? "for every account" : null,
    leftover.size === null ? "size unknown" : bytes(leftover.size),
  ]
    .filter((part) => part !== null)
    .join(", ");

/** Finder asks for a password to move what is in /Library (macOS). */
const password = (leftovers: LeftoverView[]): string =>
  leftovers.some((leftover) => leftover.systemWide)
    ? " Items for every account ask for an administrator's password when they move."
    : "";

/** The leftovers dialog's words after `name` was removed. */
export const leftoversIntro = (
  name: string,
  leftovers: LeftoverView[],
): string =>
  `Tick what to move to the ${trashName()}. Only items named after ${name}, and background jobs that start it, are listed; check each one, since another program may share a name. A background job is stopped before it moves.${password(leftovers)}`;

/** The words for the background jobs of apps already gone (macOS). */
export const orphansIntro = (leftovers: LeftoverView[]): string =>
  `Tick what to move to the ${trashName()}. These background jobs belong to apps that are no longer on this Mac: Spotlight cannot find the app they name, or the app their program was in is gone. Check each one before moving it. A background job is stopped before it moves.${password(leftovers)}`;
