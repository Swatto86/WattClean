// Decoders for what the app's commands return: every payload is checked
// here, where it enters the page.

import {
  arr,
  bool,
  num,
  obj,
  oneOf,
  optStr,
  str,
  type Fields,
} from "./decode.ts";
import {
  CATEGORIES,
  ITEM_KINDS,
  PLATFORMS,
  TEXT_SIZES,
  THEMES,
  type AppInfo,
  type ShellSettings,
  type OptionView,
  type CleanerView,
  type CommunityStatus,
  type Catalog,
  type PathEntry,
  type Settings,
  type Counts,
  type Item,
  type ProblemLine,
  type OptionReport,
  type Report,
  type Progress,
  type ShredReport,
  type ShredProgress,
  type Drive,
  type WipeReport,
  type WipeProgress,
  type HistoryEntry,
} from "./api.ts";

export const strings = (v: unknown, w: string): string[] => arr(v, w, str);

export const pathEntry = (v: unknown, w: string): PathEntry => {
  const o = obj(v, w);
  return {
    path: str(o["path"], `${w}.path`),
    folder: bool(o["folder"], `${w}.folder`),
  };
};

export const appInfo = (v: unknown): AppInfo => {
  const o = obj(v, "app info");
  return {
    version: str(o["version"], "version"),
    platform: oneOf(o["platform"], "platform", PLATFORMS),
    elevated: bool(o["elevated"], "elevated"),
    testMachine: bool(o["testMachine"], "testMachine"),
    dataDir: str(o["dataDir"], "dataDir"),
    cleanersDir: str(o["cleanersDir"], "cleanersDir"),
    windowsBuild:
      o["windowsBuild"] === null
        ? null
        : num(o["windowsBuild"], "windowsBuild"),
    startedHidden: bool(o["startedHidden"], "startedHidden"),
    adoptProblems: strings(o["adoptProblems"], "adoptProblems"),
    trayShown: bool(o["trayShown"], "trayShown"),
  };
};

export const shellSettings = (v: unknown): ShellSettings => {
  const o = obj(v, "shell settings");
  return {
    theme: oneOf(o["theme"], "theme", THEMES),
    accent: str(o["accent"], "accent"),
    textSize: oneOf(o["textSize"], "textSize", TEXT_SIZES),
    navCollapsed: bool(o["navCollapsed"], "navCollapsed"),
    keepInTray: bool(o["keepInTray"], "keepInTray"),
  };
};

export const optionView = (v: unknown, w: string): OptionView => {
  const o = obj(v, w);
  return {
    id: str(o["id"], `${w}.id`),
    name: str(o["name"], `${w}.name`),
    description: str(o["description"], `${w}.description`),
    warning: optStr(o["warning"], `${w}.warning`),
    isDefault: bool(o["default"], `${w}.default`),
  };
};

export const cleanerView = (v: unknown, w: string): CleanerView => {
  const o = obj(v, w);
  return {
    id: str(o["id"], `${w}.id`),
    name: str(o["name"], `${w}.name`),
    description: str(o["description"], `${w}.description`),
    category: oneOf(o["category"], `${w}.category`, CATEGORIES),
    source: oneOf(o["source"], `${w}.source`, [
      "built-in",
      "custom",
      "community",
    ] as const),
    present: bool(o["present"], `${w}.present`),
    running: bool(o["running"], `${w}.running`),
    options: arr(o["options"], `${w}.options`, optionView),
  };
};

export const communityStatus = (v: unknown): CommunityStatus => {
  const o = obj(v, "community");
  return {
    present: bool(o["present"], "community.present"),
    cleaners: num(o["cleaners"], "community.cleaners"),
    enabled: bool(o["enabled"], "community.enabled"),
  };
};

export const catalog = (v: unknown): Catalog => {
  const o = obj(v, "catalog");
  return {
    cleaners: arr(o["cleaners"], "cleaners", cleanerView),
    problems: strings(o["problems"], "problems"),
    selection: strings(o["selection"], "selection"),
    community: communityStatus(o["community"]),
  };
};

export const settings = (v: unknown): Settings => {
  const o = obj(v, "settings");
  return {
    theme: oneOf(o["theme"], "theme", THEMES),
    accent: str(o["accent"], "accent"),
    hideIrrelevant: bool(o["hideIrrelevant"], "hideIrrelevant"),
    shred: bool(o["shred"], "shred"),
    confirm: bool(o["confirm"], "confirm"),
    deleteOnReboot: bool(o["deleteOnReboot"], "deleteOnReboot"),
    community: bool(o["community"], "community"),
    exitAfterClean: bool(o["exitAfterClean"], "exitAfterClean"),
    selection:
      o["selection"] === null ? null : strings(o["selection"], "selection"),
    keep: arr(o["keep"], "keep", pathEntry),
    customPaths: arr(o["customPaths"], "customPaths", pathEntry),
    navCollapsed: bool(o["navCollapsed"], "navCollapsed"),
  };
};

export const counts = (v: unknown, w: string): Counts => {
  const o = obj(v, w);
  const n = (key: string) => num(o[key], `${w}.${key}`);
  return {
    files: n("files"),
    folders: n("folders"),
    bytes: n("bytes"),
    registry: n("registry"),
    other: n("other"),
    problems: n("problems"),
    inUse: n("inUse"),
  };
};

export const problemLine = (v: unknown, w: string): ProblemLine => {
  const o = obj(v, w);
  return {
    path: str(o["path"], `${w}.path`),
    message: str(o["message"], `${w}.message`),
  };
};

export const item = (v: unknown, w: string): Item => {
  const o = obj(v, w);
  return {
    kind: oneOf(o["kind"], `${w}.kind`, ITEM_KINDS),
    path: str(o["path"], `${w}.path`),
    size: num(o["size"], `${w}.size`),
    note: optStr(o["note"], `${w}.note`),
  };
};

export const optionReport = (v: unknown, w: string): OptionReport => {
  const o = obj(v, w);
  return {
    key: str(o["key"], `${w}.key`),
    cleaner: str(o["cleaner"], `${w}.cleaner`),
    option: str(o["option"], `${w}.option`),
    counts: counts(o["counts"], `${w}.counts`),
    items: arr(o["items"], `${w}.items`, item),
    more: num(o["more"], `${w}.more`),
    problems: arr(o["problems"], `${w}.problems`, problemLine),
  };
};

export const report = (v: unknown): Report => {
  const o = obj(v, "report");
  return {
    mode: oneOf(o["mode"], "mode", ["preview", "clean"] as const),
    options: arr(o["options"], "options", optionReport),
    totals: counts(o["totals"], "totals"),
    cancelled: bool(o["cancelled"], "cancelled"),
    elapsedMs: num(o["elapsedMs"], "elapsedMs"),
    running: strings(o["running"], "running"),
  };
};

export const progress = (v: unknown): Progress => {
  const o = obj(v, "progress");
  return {
    label: str(o["label"], "label"),
    done: num(o["done"], "done"),
    total: num(o["total"], "total"),
    items: num(o["items"], "items"),
    bytes: num(o["bytes"], "bytes"),
  };
};

export const shredReport = (v: unknown): ShredReport => {
  const o = obj(v, "shred report");
  return {
    files: num(o["files"], "files"),
    folders: num(o["folders"], "folders"),
    bytes: num(o["bytes"], "bytes"),
    problems: arr(o["problems"], "problems", problemLine),
    cancelled: bool(o["cancelled"], "cancelled"),
    left: strings(o["left"], "left"),
  };
};

export const shredProgress = (v: unknown): ShredProgress => {
  const o = obj(v, "shred progress");
  return {
    files: num(o["files"], "files"),
    folders: num(o["folders"], "folders"),
    bytes: num(o["bytes"], "bytes"),
    current: str(o["current"], "current"),
  };
};

export const drive = (v: unknown, w: string): Drive => {
  const o = obj(v, w);
  return {
    root: str(o["root"], `${w}.root`),
    label: str(o["label"], `${w}.label`),
    fileSystem: str(o["fileSystem"], `${w}.fileSystem`),
    free: num(o["free"], `${w}.free`),
    total: num(o["total"], `${w}.total`),
    removable: bool(o["removable"], `${w}.removable`),
  };
};

export const wipeReport = (v: unknown): WipeReport => {
  const o = obj(v, "wipe report");
  return {
    written: num(o["written"], "written"),
    cancelled: bool(o["cancelled"], "cancelled"),
    elapsedMs: num(o["elapsedMs"], "elapsedMs"),
  };
};

export const wipeProgress = (v: unknown): WipeProgress => {
  const o = obj(v, "wipe progress");
  return {
    written: num(o["written"], "written"),
    target: num(o["target"], "target"),
  };
};

export const historyEntry = (v: unknown, w: string): HistoryEntry => {
  const o: Fields = obj(v, w);
  return {
    at: num(o["at"], `${w}.at`),
    activity: oneOf(o["activity"], `${w}.activity`, [
      "clean",
      "shred",
      "wipe",
    ] as const),
    summary: str(o["summary"], `${w}.summary`),
    items: num(o["items"], `${w}.items`),
    bytes: num(o["bytes"], `${w}.bytes`),
    problems: num(o["problems"], `${w}.problems`),
    cancelled: bool(o["cancelled"], `${w}.cancelled`),
  };
};
