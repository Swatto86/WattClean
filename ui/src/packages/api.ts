// The uninstaller's commands on Linux and macOS, decoded where they enter
// the page. Programs are named by the key they were listed under and
// leftovers by number: the page never names a path or command itself.

import { invoke } from "@tauri-apps/api/core";

import { arr, bool, failure, num, obj, optStr, str } from "../decode.ts";

export interface PackageView {
  key: string;
  name: string;
  version: string | null;
  size: number | null;
  summary: string | null;
  app: boolean;
  /** Where it came from: APT, Flatpak, Homebrew Cask… */
  source: string;
}

export interface PackageList {
  packages: PackageView[];
  problems: string[];
}

export interface LeftoverView {
  number: number;
  path: string;
  /** Null when the system would not let WattClean look inside. */
  size: number | null;
  folder: boolean;
  /** A background job's plist or helper: its job is stopped before it
   *  moves. */
  job: boolean;
  /** In /Library, for every account: moving it asks for a password. */
  systemWide: boolean;
}

export interface Uninstalled {
  name: string;
  said: string | null;
  leftovers: LeftoverView[];
}

/** The background jobs of apps already gone (macOS). */
export interface Orphans {
  leftovers: LeftoverView[];
  /** False when Spotlight could not be asked: only jobs whose app folder
   *  is gone were found. */
  spotlight: boolean;
}

export interface Trashed {
  moved: number;
  failed: string[];
}

const optNum = (v: unknown, w: string): number | null =>
  v === null || v === undefined ? null : num(v, w);

const packageView = (v: unknown, w: string): PackageView => {
  const o = obj(v, w);
  return {
    key: str(o["key"], `${w}.key`),
    name: str(o["name"], `${w}.name`),
    version: optStr(o["version"], `${w}.version`),
    size: optNum(o["size"], `${w}.size`),
    summary: optStr(o["summary"], `${w}.summary`),
    app: bool(o["app"], `${w}.app`),
    source: str(o["source"], `${w}.source`),
  };
};

const leftoverView = (v: unknown, w: string): LeftoverView => {
  const o = obj(v, w);
  return {
    number: num(o["number"], `${w}.number`),
    path: str(o["path"], `${w}.path`),
    size: optNum(o["size"], `${w}.size`),
    folder: bool(o["folder"], `${w}.folder`),
    job: bool(o["job"], `${w}.job`),
    systemWide: bool(o["systemWide"], `${w}.systemWide`),
  };
};

export const decodeList = (v: unknown): PackageList => {
  const o = obj(v, "packages");
  return {
    packages: arr(o["packages"], "packages", packageView),
    problems: arr(o["problems"], "problems", str),
  };
};

export const decodeUninstalled = (v: unknown): Uninstalled => {
  const o = obj(v, "uninstalled");
  return {
    name: str(o["name"], "name"),
    said: optStr(o["said"], "said"),
    leftovers: arr(o["leftovers"], "leftovers", leftoverView),
  };
};

export const decodeOrphans = (v: unknown): Orphans => {
  const o = obj(v, "orphans");
  return {
    leftovers: arr(o["leftovers"], "leftovers", leftoverView),
    spotlight: bool(o["spotlight"], "spotlight"),
  };
};

export const decodeTrashed = (v: unknown): Trashed => {
  const o = obj(v, "trashed");
  return {
    moved: num(o["moved"], "moved"),
    failed: arr(o["failed"], "failed", str),
  };
};

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

export const packagesApi = {
  list: (all: boolean) => call("un_pkg_list", decodeList, { all }),
  uninstall: (key: string, purge: boolean) =>
    call("un_pkg_uninstall", decodeUninstalled, { key, purge }),
  orphans: () => call("un_pkg_orphans", decodeOrphans),
  trashLeftovers: (numbers: number[]) =>
    call("un_pkg_trash_leftovers", decodeTrashed, { numbers }),
  reveal: (number: number) =>
    call("un_pkg_reveal", () => undefined, { number }),
};
