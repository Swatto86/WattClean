// The one typed wrapper over the app's commands. Every result is decoded
// here, where it enters the page; components never call `invoke` directly.

import { Channel, invoke } from "@tauri-apps/api/core";

import { arr, failure, str } from "./decode.ts";
import * as wire from "./wire.ts";

export const CATEGORIES = [
  "system",
  "packages",
  "browsers",
  "applications",
  "developer",
  "games",
  "deep-scan",
  "custom",
  "community",
] as const;
export type Category = (typeof CATEGORIES)[number];
export type Mode = "preview" | "clean";
export const THEMES = ["system", "light", "dark"] as const;
export type Theme = (typeof THEMES)[number];
export const ITEM_KINDS = [
  "file",
  "folder",
  "truncate",
  "registry",
  "database",
  "json",
  "ini",
  "recycle-bin",
  "clipboard",
  "free-space",
  "command",
] as const;
export type ItemKind = (typeof ITEM_KINDS)[number];
export const PLATFORMS = ["windows", "linux", "macos"] as const;
export type Platform = (typeof PLATFORMS)[number];

export interface AppInfo {
  version: string;
  /** Which modules exist and what the system's things are called. */
  platform: Platform;
  elevated: boolean;
  testMachine: boolean;
  /** WattClean's data folder; each module has its own folder inside. */
  dataDir: string;
  cleanersDir: string;
  windowsBuild: number | null;
  /** Started at sign-in: the window stays hidden until opened from the tray. */
  startedHidden: boolean;
  /** What could not be copied from the earlier apps at this start. */
  adoptProblems: string[];
}

export const TEXT_SIZES = ["small", "normal", "large"] as const;
export type TextSize = (typeof TEXT_SIZES)[number];

/** The shell's settings: the look, the navigation pane, the tray. */
export interface ShellSettings {
  theme: Theme;
  accent: string;
  textSize: TextSize;
  navCollapsed: boolean;
  keepInTray: boolean;
}

export type ShellPatch = Partial<ShellSettings>;

export interface OptionView {
  id: string;
  name: string;
  description: string;
  warning: string | null;
  isDefault: boolean;
}

export interface CleanerView {
  id: string;
  name: string;
  description: string;
  category: Category;
  source: "built-in" | "custom" | "community";
  present: boolean;
  running: boolean;
  options: OptionView[];
}

export interface CommunityStatus {
  present: boolean;
  cleaners: number;
  enabled: boolean;
}

export interface Catalog {
  cleaners: CleanerView[];
  problems: string[];
  selection: string[];
  community: CommunityStatus;
}

export interface PathEntry {
  path: string;
  folder: boolean;
}

export interface Settings {
  theme: Theme;
  accent: string;
  hideIrrelevant: boolean;
  shred: boolean;
  confirm: boolean;
  deleteOnReboot: boolean;
  community: boolean;
  exitAfterClean: boolean;
  selection: string[] | null;
  keep: PathEntry[];
  customPaths: PathEntry[];
  navCollapsed: boolean;
}

export type SettingsPatch = Partial<Settings>;

export interface Counts {
  files: number;
  folders: number;
  bytes: number;
  registry: number;
  other: number;
  problems: number;
  inUse: number;
}

export interface Item {
  kind: ItemKind;
  path: string;
  size: number;
  note: string | null;
}

export interface ProblemLine {
  path: string;
  message: string;
}

export interface OptionReport {
  key: string;
  cleaner: string;
  option: string;
  counts: Counts;
  items: Item[];
  more: number;
  problems: ProblemLine[];
}

export interface Report {
  mode: Mode;
  options: OptionReport[];
  totals: Counts;
  cancelled: boolean;
  elapsedMs: number;
  running: string[];
}

export interface Progress {
  label: string;
  done: number;
  total: number;
  items: number;
  bytes: number;
}

export interface ShredReport {
  files: number;
  folders: number;
  bytes: number;
  problems: ProblemLine[];
  cancelled: boolean;
}

export interface ShredProgress {
  files: number;
  folders: number;
  bytes: number;
  current: string;
}

export interface Drive {
  root: string;
  label: string;
  fileSystem: string;
  free: number;
  total: number;
  removable: boolean;
}

export interface WipeReport {
  written: number;
  cancelled: boolean;
  elapsedMs: number;
}

export interface WipeProgress {
  written: number;
  target: number;
}

export interface HistoryEntry {
  at: number;
  activity: "clean" | "shred" | "wipe";
  summary: string;
  items: number;
  bytes: number;
  problems: number;
  cancelled: boolean;
}

// Commands

const call = async <T>(
  command: string,
  decode: (value: unknown) => T,
  args?: Record<string, unknown>,
): Promise<T> => {
  let value: unknown;
  try {
    value = await invoke(command, args);
  } catch (error) {
    throw new Error(failure(error));
  }
  return decode(value);
};

const nothing = (): void => undefined;

/** A channel whose messages are decoded before `onMessage` sees them. */
const channel = <T>(
  decode: (v: unknown) => T,
  onMessage: (value: T) => void,
): Channel<unknown> => {
  const ch = new Channel<unknown>();
  ch.onmessage = (message) => {
    try {
      onMessage(decode(message));
    } catch (error) {
      console.error(error);
    }
  };
  return ch;
};

export const api = {
  appInfo: () => call("app_info", wire.appInfo),
  getShellSettings: () => call("get_shell_settings", wire.shellSettings),
  updateShellSettings: (patch: ShellPatch) =>
    call("update_shell_settings", wire.shellSettings, { patch }),
  openDataFolder: () => call("open_data_folder", nothing),
  getSettings: () => call("disk_get_settings", wire.settings),
  updateSettings: (patch: SettingsPatch) =>
    call("disk_update_settings", wire.settings, { patch }),
  pickPath: (folder: boolean) =>
    call("disk_pick_path", (v) => (v === null ? null : str(v, "path")), {
      folder,
    }),
  getCatalog: () => call("disk_get_catalog", wire.catalog),
  runCleaner: (
    mode: Mode,
    selection: string[],
    onProgress: (p: Progress) => void,
  ) =>
    call("disk_run_cleaner", wire.report, {
      mode,
      selection,
      onProgress: channel(wire.progress, onProgress),
    }),
  cancelJob: () => call("disk_cancel_job", nothing),
  reloadCatalog: () => call("disk_reload_catalog", nothing),
  pickToShred: (folder: boolean) =>
    call("disk_pick_to_shred", (v) => wire.strings(v, "paths"), { folder }),
  shred: (paths: string[], onProgress: (p: ShredProgress) => void) =>
    call("disk_shred", wire.shredReport, {
      paths,
      onProgress: channel(wire.shredProgress, onProgress),
    }),
  getDrives: () => call("disk_get_drives", (v) => arr(v, "drives", wire.drive)),
  wipeFreeSpace: (root: string, onProgress: (p: WipeProgress) => void) =>
    call("disk_wipe_free_space", wire.wipeReport, {
      root,
      onProgress: channel(wire.wipeProgress, onProgress),
    }),
  importWinapp2: () =>
    call("disk_import_winapp2", (v) =>
      v === null ? null : wire.communityStatus(v),
    ),
  downloadWinapp2: () => call("disk_download_winapp2", wire.communityStatus),
  removeWinapp2: () => call("disk_remove_winapp2", wire.communityStatus),
  getHistory: () =>
    call("disk_get_history", (v) => arr(v, "history", wire.historyEntry)),
  clearHistory: () => call("disk_clear_history", nothing),
  openCleanersFolder: () => call("disk_open_cleaners_folder", nothing),
  restartAsAdmin: () => call("restart_as_admin", nothing),
  quitApp: () => call("quit_app", nothing),
};
