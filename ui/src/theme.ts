// Light or dark (following Windows when set to System), the accent, and
// each page's own colour, written as CSS custom properties.

import { getCurrentWindow } from "@tauri-apps/api/window";

import type { Theme } from "./api.ts";

/** Named accents as light and dark variants (WattTask's set). */
export const ACCENTS: Record<
  string,
  { name: string; light: string; dark: string }
> = {
  sky: { name: "Sky blue", light: "#0067c0", dark: "#4cc2ff" },
  violet: { name: "Violet", light: "#7a3cdb", dark: "#b691ff" },
  pink: { name: "Pink", light: "#c4247a", dark: "#ff7ac0" },
  teal: { name: "Teal", light: "#0f8a7e", dark: "#3ee0c8" },
  green: { name: "Green", light: "#1a7f37", dark: "#5fe07a" },
  amber: { name: "Amber", light: "#b45a00", dark: "#ffb43a" },
  red: { name: "Red", light: "#c42b1c", dark: "#ff6b6b" },
};

/** Each page's colour for its icon and title. */
export const PAGE_COLOURS: Record<string, { light: string; dark: string }> = {
  cleaner: { light: "#0a84ff", dark: "#3ab4ff" },
  shred: { light: "#dc2626", dark: "#ff6b6b" },
  wipe: { light: "#0d9488", dark: "#2de0c8" },
  history: { light: "#8b3dff", dark: "#b98cff" },
  registry: { light: "#2563eb", dark: "#60a5fa" },
  regsearch: { light: "#0891b2", dark: "#22d3ee" },
  tuner: { light: "#ca8a04", dark: "#facc15" },
  watcher: { light: "#ea580c", dark: "#fb923c" },
  regbackups: { light: "#16a34a", dark: "#4ade80" },
  schedules: { light: "#0d9488", dark: "#2dd4bf" },
  programs: { light: "#7c3aed", dark: "#a78bfa" },
  traced: { light: "#db2777", dark: "#f472b6" },
  forced: { light: "#dc2626", dark: "#f87171" },
  unbackups: { light: "#16a34a", dark: "#4ade80" },
  unhistory: { light: "#8b3dff", dark: "#b98cff" },
  settings: { light: "#64748b", dark: "#a7b4c8" },
};

const media = window.matchMedia("(prefers-color-scheme: dark)");

export const isDark = (theme: Theme): boolean =>
  theme === "dark" || (theme === "system" && media.matches);

const rgb = (hex: string): string => {
  const value = Number.parseInt(hex.slice(1), 16);
  return `${(value >> 16) & 255} ${(value >> 8) & 255} ${value & 255}`;
};

/** Applies the theme and accent to the page and the window frame. */
export const applyTheme = (theme: Theme, accent: string): void => {
  const dark = isDark(theme);
  const root = document.documentElement;
  root.dataset["theme"] = dark ? "dark" : "light";
  const chosen = ACCENTS[accent] ?? ACCENTS["sky"];
  if (chosen) {
    const colour = dark ? chosen.dark : chosen.light;
    root.style.setProperty("--accent", colour);
    root.style.setProperty("--accent-rgb", rgb(colour));
    root.style.setProperty("--accent-text", dark ? "#000000" : "#ffffff");
  }
  for (const [page, colours] of Object.entries(PAGE_COLOURS)) {
    root.style.setProperty(
      `--page-${page}`,
      dark ? colours.dark : colours.light,
    );
  }
  // The title bar follows; a failure here only leaves its colour.
  getCurrentWindow()
    .setTheme(theme === "system" ? null : dark ? "dark" : "light")
    .catch(() => undefined);
};

/** Calls back when Windows switches between light and dark. */
export const onSystemThemeChange = (callback: () => void): void => {
  media.addEventListener("change", callback);
};
