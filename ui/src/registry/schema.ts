/** The shapes the Rust commands return, each checked as it arrives. */
import {
  bool,
  list,
  num,
  oneOf,
  optNum,
  optStr,
  record,
  str,
} from "./decode.ts";

export const CATEGORIES = [
  "startup",
  "appPaths",
  "cache",
  "missingSharedDlls",
  "uninstall",
  "contextMenus",
  "comObjects",
  "msiFolders",
  "firewall",
  "systemSettings",
  "extensions",
  "unusedSharedDlls",
  "services",
  "emptyKeys",
  "classKeys",
  "history",
  "includeList",
  "search",
] as const;
export type Category = (typeof CATEGORIES)[number];

export const MODES = ["safe", "moderate", "advanced", "custom"] as const;
export type Mode = (typeof MODES)[number];

export const THEMES = ["system", "light", "dark"] as const;
export const FONT_SIZES = ["small", "normal", "large"] as const;
export type Theme = (typeof THEMES)[number];

export const HIVES = ["HKLM", "HKCU", "HKU"] as const;
export interface RegKey {
  hive: (typeof HIVES)[number];
  path: string;
}

/** `HKCU\Software\X`, as the registry tools write it. */
export function keyText(key: RegKey): string {
  return key.path === "" ? key.hive : `${key.hive}\\${key.path}`;
}

export function regKey(v: unknown, w: string): RegKey {
  const r = record(v, w);
  return {
    hive: oneOf(r["hive"], `${w}.hive`, HIVES),
    path: str(r["path"], `${w}.path`),
  };
}

export interface Finding {
  id: number;
  category: Category;
  key: RegKey;
  value: string | null;
  detail: string;
  account: string | null;
  ticked: boolean;
  needsAdmin: boolean;
}

export function finding(v: unknown, w: string): Finding {
  const r = record(v, w);
  return {
    id: num(r["id"], `${w}.id`),
    category: oneOf(r["category"], `${w}.category`, CATEGORIES),
    key: regKey(r["key"], `${w}.key`),
    value: optStr(r["value"], `${w}.value`),
    detail: str(r["detail"], `${w}.detail`),
    account: optStr(r["account"], `${w}.account`),
    ticked: bool(r["ticked"], `${w}.ticked`),
    needsAdmin: bool(r["needsAdmin"], `${w}.needsAdmin`),
  };
}

export interface Report {
  findings: Finding[];
  notes: string[];
  cancelled: boolean;
}

export function report(v: unknown, w: string): Report {
  const r = record(v, w);
  return {
    findings: list(r["findings"], `${w}.findings`, finding),
    notes: list(r["notes"], `${w}.notes`, str),
    cancelled: bool(r["cancelled"], `${w}.cancelled`),
  };
}

export interface CategoryInfo {
  id: Category;
  title: string;
  description: string;
  fromMode: Mode | null;
}

export function categoryInfo(v: unknown, w: string): CategoryInfo {
  const r = record(v, w);
  const from = optStr(r["fromMode"], `${w}.fromMode`);
  return {
    id: oneOf(r["id"], `${w}.id`, CATEGORIES),
    title: str(r["title"], `${w}.title`),
    description: str(r["description"], `${w}.description`),
    fromMode: from === null ? null : oneOf(from, `${w}.fromMode`, MODES),
  };
}

export interface ListEntry {
  key: string;
  subkeys: boolean;
  enabled: boolean;
}

function listEntry(v: unknown, w: string): ListEntry {
  const r = record(v, w);
  return {
    key: str(r["key"], `${w}.key`),
    subkeys: bool(r["subkeys"], `${w}.subkeys`),
    enabled: bool(r["enabled"], `${w}.enabled`),
  };
}

export interface Settings {
  version: number;
  theme: Theme;
  mode: Mode;
  custom: Category[];
  allAccounts: boolean;
  excludeRemovable: boolean;
  excludeWindows: boolean;
  restorePoint: boolean;
  forceDelete: boolean;
  fullBackup: boolean;
  fontSize: (typeof FONT_SIZES)[number];
  backupsFolder: string | null;
  keepInTray: boolean;
  keepBackupsDays: number;
  keepBackupsMb: number;
  excludes: ListEntry[];
  includes: ListEntry[];
}

export function settings(v: unknown, w: string): Settings {
  const r = record(v, w);
  return {
    version: num(r["version"], `${w}.version`),
    theme: oneOf(r["theme"], `${w}.theme`, THEMES),
    mode: oneOf(r["mode"], `${w}.mode`, MODES),
    custom: list(r["custom"], `${w}.custom`, (c, cw) =>
      oneOf(c, cw, CATEGORIES),
    ),
    allAccounts: bool(r["allAccounts"], `${w}.allAccounts`),
    excludeRemovable: bool(r["excludeRemovable"], `${w}.excludeRemovable`),
    excludeWindows: bool(r["excludeWindows"], `${w}.excludeWindows`),
    restorePoint: bool(r["restorePoint"], `${w}.restorePoint`),
    forceDelete: bool(r["forceDelete"], `${w}.forceDelete`),
    fullBackup: bool(r["fullBackup"], `${w}.fullBackup`),
    fontSize: oneOf(r["fontSize"], `${w}.fontSize`, FONT_SIZES),
    backupsFolder: optStr(r["backupsFolder"], `${w}.backupsFolder`),
    keepInTray: bool(r["keepInTray"], `${w}.keepInTray`),
    keepBackupsDays: num(r["keepBackupsDays"], `${w}.keepBackupsDays`),
    keepBackupsMb: num(r["keepBackupsMb"], `${w}.keepBackupsMb`),
    excludes: list(r["excludes"], `${w}.excludes`, listEntry),
    includes: list(r["includes"], `${w}.includes`, listEntry),
  };
}

export interface AppInfo {
  version: string;
  elevated: boolean;
  testMachine: boolean;
  dataDir: string;
  windowsBuild: number | null;
  startedHidden: boolean;
}

export function appInfo(v: unknown, w: string): AppInfo {
  const r = record(v, w);
  return {
    version: str(r["version"], `${w}.version`),
    elevated: bool(r["elevated"], `${w}.elevated`),
    testMachine: bool(r["testMachine"], `${w}.testMachine`),
    dataDir: str(r["dataDir"], `${w}.dataDir`),
    windowsBuild: optNum(r["windowsBuild"], `${w}.windowsBuild`),
    startedHidden: bool(r["startedHidden"], `${w}.startedHidden`),
  };
}

/** `[what, why]` pairs, as Rust's `Vec<(String, String)>` arrives. */
export function pairs(v: unknown, w: string): [string, string][] {
  return list(v, w, (p, pw) => {
    const both = list(p, pw, str);
    const [first, second] = both;
    if (both.length !== 2 || first === undefined || second === undefined) {
      throw new Error(`${pw} is not a pair`);
    }
    return [first, second];
  });
}

export interface CleanOutcome {
  backup: string | null;
  removed: number;
  forced: number;
  warnings: [string, string][];
  failed: [string, string][];
  removedIds: number[];
  restorePointError: string | null;
  fullBackupError: string | null;
}

export function cleanOutcome(v: unknown, w: string): CleanOutcome {
  const r = record(v, w);
  return {
    backup: optStr(r["backup"], `${w}.backup`),
    removed: num(r["removed"], `${w}.removed`),
    forced: num(r["forced"], `${w}.forced`),
    warnings: pairs(r["warnings"], `${w}.warnings`),
    failed: pairs(r["failed"], `${w}.failed`),
    removedIds: list(r["removedIds"], `${w}.removedIds`, num),
    restorePointError: optStr(r["restorePointError"], `${w}.restorePointError`),
    fullBackupError: optStr(r["fullBackupError"], `${w}.fullBackupError`),
  };
}

export function nothing(): void {
  return undefined;
}
