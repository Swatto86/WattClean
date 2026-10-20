// What the uninstaller tells people about a removal on Linux and macOS,
// by where the program came from.

import { platform, trashName } from "../platform.ts";

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
