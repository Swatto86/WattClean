/** The shapes Rust commands return, and the checks that prove them. */
import {
  bool,
  list,
  num,
  oneOf,
  optNum,
  optStr,
  record,
  str,
  type Fields,
} from "./decode.ts";

export const MODES = ["safe", "moderate", "advanced"] as const;
export type Mode = (typeof MODES)[number];
export const THEMES = ["system", "light", "dark"] as const;
export type Theme = (typeof THEMES)[number];
export type Confidence = "low" | "medium" | "high";

export interface RegPath {
  hive: "HKLM" | "HKCU" | "HKU";
  path: string;
}

export interface Program {
  id: string;
  name: string;
  version: string | null;
  publisher: string | null;
  installDate: string | null;
  sizeKb: number | null;
  installLocation: string | null;
  uninstallCommand: string | null;
  website: string | null;
  comments: string | null;
  registryKey: RegPath;
  scope: "machine64" | "machine32" | "user";
  msiProductCode: string | null;
  systemComponent: boolean;
}

export interface Settings {
  version: number;
  mode: Mode;
  restorePoint: boolean;
  showSystemComponents: boolean;
  quiet: boolean;
  theme: Theme;
}

export interface AppInfo {
  version: string;
  elevated: boolean;
  testMachine: boolean;
  dataDir: string;
}

export interface SessionView {
  id: number;
  name: string;
  publisher: string | null;
  version: string | null;
  forced: boolean;
  installDirs: string[];
  executables: number;
  uninstallCommand: string | null;
  needsAdmin: boolean;
  /// Items the program's install log will check, when it has one.
  loggedItems: number | null;
}

export type Target =
  | { kind: "key"; key: RegPath }
  | { kind: "value"; key: RegPath; name: string }
  | { kind: "folder"; path: string }
  | { kind: "file"; path: string };

export interface Leftover {
  id: number;
  target: Target;
  reason: string;
  confidence: Confidence;
  selected: boolean;
  bytes: number;
}

export interface ScanReport {
  mode: Mode;
  items: Leftover[];
  warnings: string[];
}

export interface Counts {
  keys: number;
  values: number;
  folders: number;
  files: number;
}

export interface Failed {
  item: string;
  error: string;
}

export interface Removal {
  removed: Counts;
  atRestart: number;
  failed: Failed[];
  bytes: number;
  backupId: string;
}

export interface BackupSummary {
  id: string;
  created: number;
  program: string;
  items: number;
  bytes: number;
  restored: number | null;
  restorable: boolean;
}

export interface HistoryEntry {
  id: string;
  time: number;
  program: string;
  version: string | null;
  mode: Mode | null;
  uninstaller: {
    state: "finished" | "failed" | "notRun";
    error: string | null;
  };
  removed: Counts;
  atRestart: number;
  failed: number;
  bytes: number;
  backupId: string | null;
}

export const regPath = (v: unknown, w: string): RegPath => {
  const o = record(v, w);
  return {
    hive: oneOf(o["hive"], `${w}.hive`, ["HKLM", "HKCU", "HKU"] as const),
    path: str(o["path"], `${w}.path`),
  };
};

export const program = (v: unknown, w: string): Program => {
  const o = record(v, w);
  const text = (name: string) => optStr(o[name], `${w}.${name}`);
  return {
    id: str(o["id"], `${w}.id`),
    name: str(o["name"], `${w}.name`),
    version: text("version"),
    publisher: text("publisher"),
    installDate: text("installDate"),
    sizeKb: optNum(o["sizeKb"], `${w}.sizeKb`),
    installLocation: text("installLocation"),
    uninstallCommand: text("uninstallCommand"),
    website: text("website"),
    comments: text("comments"),
    registryKey: regPath(o["registryKey"], `${w}.registryKey`),
    scope: oneOf(o["scope"], `${w}.scope`, [
      "machine64",
      "machine32",
      "user",
    ] as const),
    msiProductCode: text("msiProductCode"),
    systemComponent: bool(o["systemComponent"], `${w}.systemComponent`),
  };
};

export const settings = (v: unknown, w: string): Settings => {
  const o = record(v, w);
  return {
    version: num(o["version"], `${w}.version`),
    mode: oneOf(o["mode"], `${w}.mode`, MODES),
    restorePoint: bool(o["restorePoint"], `${w}.restorePoint`),
    showSystemComponents: bool(
      o["showSystemComponents"],
      `${w}.showSystemComponents`,
    ),
    quiet: bool(o["quiet"], `${w}.quiet`),
    theme: oneOf(o["theme"], `${w}.theme`, THEMES),
  };
};

export const sessionView = (v: unknown, w: string): SessionView => {
  const o = record(v, w);
  return {
    id: num(o["id"], `${w}.id`),
    name: str(o["name"], `${w}.name`),
    publisher: optStr(o["publisher"], `${w}.publisher`),
    version: optStr(o["version"], `${w}.version`),
    forced: bool(o["forced"], `${w}.forced`),
    installDirs: list(o["installDirs"], `${w}.installDirs`, str),
    executables: num(o["executables"], `${w}.executables`),
    uninstallCommand: optStr(o["uninstallCommand"], `${w}.uninstallCommand`),
    needsAdmin: bool(o["needsAdmin"], `${w}.needsAdmin`),
    loggedItems: optNum(o["loggedItems"], `${w}.loggedItems`),
  };
};

export const target = (v: unknown, w: string): Target => {
  const o = record(v, w);
  const kind = oneOf(o["kind"], `${w}.kind`, [
    "key",
    "value",
    "folder",
    "file",
  ] as const);
  if (kind === "key") return { kind, key: regPath(o["key"], `${w}.key`) };
  if (kind === "value")
    return {
      kind,
      key: regPath(o["key"], `${w}.key`),
      name: str(o["name"], `${w}.name`),
    };
  return { kind, path: str(o["path"], `${w}.path`) };
};

export const leftover = (v: unknown, w: string): Leftover => {
  const o = record(v, w);
  return {
    id: num(o["id"], `${w}.id`),
    target: target(o["target"], `${w}.target`),
    reason: str(o["reason"], `${w}.reason`),
    confidence: oneOf(o["confidence"], `${w}.confidence`, [
      "low",
      "medium",
      "high",
    ] as const),
    selected: bool(o["selected"], `${w}.selected`),
    bytes: num(o["bytes"], `${w}.bytes`),
  };
};

export const report = (v: unknown, w: string): ScanReport => {
  const o = record(v, w);
  return {
    mode: oneOf(o["mode"], `${w}.mode`, MODES),
    items: list(o["items"], `${w}.items`, leftover),
    warnings: list(o["warnings"], `${w}.warnings`, str),
  };
};

export const counts = (v: unknown, w: string): Counts => {
  const o = record(v, w);
  const n = (name: string) => num(o[name], `${w}.${name}`);
  return {
    keys: n("keys"),
    values: n("values"),
    folders: n("folders"),
    files: n("files"),
  };
};

export const failed = (v: unknown, w: string): Failed => {
  const o = record(v, w);
  return {
    item: str(o["item"], `${w}.item`),
    error: str(o["error"], `${w}.error`),
  };
};

export const removal = (v: unknown, w: string): Removal => {
  const o = record(v, w);
  return {
    removed: counts(o["removed"], `${w}.removed`),
    atRestart: num(o["atRestart"], `${w}.atRestart`),
    failed: list(o["failed"], `${w}.failed`, failed),
    bytes: num(o["bytes"], `${w}.bytes`),
    backupId: str(o["backupId"], `${w}.backupId`),
  };
};

export const backupSummary = (v: unknown, w: string): BackupSummary => {
  const o = record(v, w);
  return {
    id: str(o["id"], `${w}.id`),
    created: num(o["created"], `${w}.created`),
    program: str(o["program"], `${w}.program`),
    items: num(o["items"], `${w}.items`),
    bytes: num(o["bytes"], `${w}.bytes`),
    restored: optNum(o["restored"], `${w}.restored`),
    restorable: bool(o["restorable"], `${w}.restorable`),
  };
};

export const historyEntry = (v: unknown, w: string): HistoryEntry => {
  const o = record(v, w);
  const outcome: Fields = record(o["uninstaller"], `${w}.uninstaller`);
  return {
    id: str(o["id"], `${w}.id`),
    time: num(o["time"], `${w}.time`),
    program: str(o["program"], `${w}.program`),
    version: optStr(o["version"], `${w}.version`),
    mode:
      o["mode"] === null || o["mode"] === undefined
        ? null
        : oneOf(o["mode"], `${w}.mode`, MODES),
    uninstaller: {
      state: oneOf(outcome["state"], `${w}.uninstaller.state`, [
        "finished",
        "failed",
        "notRun",
      ] as const),
      error: optStr(outcome["error"], `${w}.uninstaller.error`),
    },
    removed: counts(o["removed"], `${w}.removed`),
    atRestart: num(o["atRestart"], `${w}.atRestart`),
    failed: num(o["failed"], `${w}.failed`),
    bytes: num(o["bytes"], `${w}.bytes`),
    backupId: optStr(o["backupId"], `${w}.backupId`),
  };
};

export const appInfo = (v: unknown, w: string): AppInfo => {
  const o = record(v, w);
  return {
    version: str(o["version"], `${w}.version`),
    elevated: bool(o["elevated"], `${w}.elevated`),
    testMachine: bool(o["testMachine"], `${w}.testMachine`),
    dataDir: str(o["dataDir"], `${w}.dataDir`),
  };
};

export const nothing = (): void => undefined;

export const restoreView = (v: unknown, w: string) => {
  const o = record(v, w);
  return {
    restored: num(o["restored"], `${w}.restored`),
    failed: list(o["failed"], `${w}.failed`, failed),
  };
};
