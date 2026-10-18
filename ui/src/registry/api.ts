/** Typed wrappers for every Rust command; results are checked on arrival. */
import { invoke } from "@tauri-apps/api/core";

import { list, num, optStr, str } from "./decode.ts";
import type { MenuEntry } from "./menu.ts";
import {
  appInfo,
  categoryInfo,
  cleanOutcome,
  nothing,
  regKey,
  report,
  settings,
  type Category,
  type Mode,
  type Settings,
} from "./schema.ts";
import {
  events,
  itemView,
  resetOutcome,
  restoreOutcome,
  summary,
  traceState,
  tweak,
  watchView,
} from "./schema-records.ts";

import { background, job, lastRun, type JobInput } from "./schema-jobs.ts";

export * from "./schema.ts";
export * from "./schema-records.ts";
export * from "./schema-jobs.ts";

/** Which results a clean or export means. */
export type ResultList = "cleaner" | "search";

export interface SearchRequest {
  text: string;
  within: string;
  keys: boolean;
  values: boolean;
  data: boolean;
  matchCase: boolean;
  whole: boolean;
}

/** Run a command; a Rust error arrives as `{ message }` and becomes an Error. */
async function call<T>(
  command: string,
  args: Record<string, unknown>,
  decode: (v: unknown, w: string) => T,
): Promise<T> {
  let result: unknown;
  try {
    result = await invoke(command, args);
  } catch (error: unknown) {
    const message =
      typeof error === "object" && error !== null && "message" in error
        ? String(error.message)
        : String(error);
    throw new Error(message);
  }
  return decode(result, command);
}

export const api = {
  appInfo: () => call("app_info", {}, appInfo),
  getSettings: () => call("reg_get_settings", {}, settings),
  saveSettings: (value: Settings) =>
    call("reg_save_settings", { settings: value }, nothing),
  openInRegedit: (key: string) => call("reg_open_in_regedit", { key }, nothing),
  restartExplorer: () => call("reg_restart_explorer", {}, nothing),
  restartAsAdmin: () => call("restart_as_admin", {}, nothing),
  signOut: () => call("reg_sign_out", {}, nothing),
  restartPc: () => call("reg_restart_pc", {}, nothing),
  backUpFull: () => call("reg_back_up_full", {}, summary),
  exportSettings: () => call("reg_export_settings", {}, optStr),
  importSettings: () =>
    call("reg_import_settings", {}, (v, w) =>
      v === null || v === undefined ? null : settings(v, w),
    ),
  chooseBackupsFolder: () => call("reg_choose_backups_folder", {}, optStr),
  defaultBackupsFolder: () => call("reg_default_backups_folder", {}, str),
  searchWeb: (text: string) => call("reg_search_web", { text }, nothing),
  contextMenu: (entries: MenuEntry[], x: number, y: number) =>
    call("reg_context_menu", { entries, x, y }, optStr),

  categories: () =>
    call("reg_categories", {}, (v, w) => list(v, w, categoryInfo)),
  scanRegistry: (mode: Mode, categories: Category[], allAccounts: boolean) =>
    call(
      "reg_scan_registry",
      { request: { mode, categories, allAccounts } },
      report,
    ),
  searchRegistry: (request: SearchRequest) =>
    call("reg_search_registry", { request }, report),
  scanProgress: () => call("reg_scan_progress", {}, str),
  cancelScan: () => call("reg_cancel_scan", {}, nothing),
  clean: (
    list: ResultList,
    ids: number[],
    restorePoint: boolean,
    force: boolean,
    fullBackup: boolean,
  ) =>
    call(
      "reg_clean",
      { list, ids, restorePoint, force, fullBackup },
      cleanOutcome,
    ),
  exportFindings: (list: ResultList) =>
    call("reg_export_findings", { list }, optStr),

  listBackups: () =>
    call("reg_list_backups", {}, (v, w) => list(v, w, summary)),
  backupItems: (id: string) =>
    call("reg_backup_items", { id }, (v, w) => list(v, w, itemView)),
  restoreBackup: (id: string, items: number[] | null) =>
    call("reg_restore_backup", { id, items }, restoreOutcome),
  deleteBackup: (id: string) => call("reg_delete_backup", { id }, nothing),
  deleteAllBackups: () => call("reg_delete_all_backups", {}, num),
  exportBackup: (id: string) => call("reg_export_backup", { id }, optStr),
  backUpKey: (key: string, label: string) =>
    call("reg_back_up_key", { key, label }, summary),

  listTweaks: () => call("reg_list_tweaks", {}, (v, w) => list(v, w, tweak)),
  setTweak: (id: string, on: boolean) =>
    call("reg_set_tweak", { id, on }, tweak),
  resetTweaks: () => call("reg_reset_tweaks", {}, resetOutcome),

  listWatches: () =>
    call("reg_list_watches", {}, (v, w) => list(v, w, watchView)),
  addWatch: (key: string) => call("reg_add_watch", { key }, regKey),
  removeWatch: (key: string) => call("reg_remove_watch", { key }, nothing),
  pauseWatch: (key: string, paused: boolean) =>
    call("reg_pause_watch", { key, paused }, nothing),
  watchEvents: (key: string, since: number) =>
    call("reg_watch_events", { key, since }, events),
  clearWatch: (key: string) => call("reg_clear_watch", { key }, nothing),
  exportWatch: (key: string) => call("reg_export_watch", { key }, optStr),
  watchTracing: () => call("reg_watch_tracing", {}, traceState),

  listJobs: () => call("reg_list_jobs", {}, (v, w) => list(v, w, job)),
  saveJob: (input: JobInput) => call("reg_save_job", { job: input }, job),
  setJobEnabled: (id: string, enabled: boolean) =>
    call("reg_set_job_enabled", { id, enabled }, job),
  deleteJob: (id: string) => call("reg_delete_job", { id }, nothing),
  runJobNow: (id: string) => call("reg_run_job_now", { id }, lastRun),
  backgroundState: () => call("reg_background_state", {}, background),
  setStartWithWindows: (on: boolean) =>
    call("reg_set_start_with_windows", { on }, background),
};

export const message = (error: unknown): string =>
  error instanceof Error ? error.message : String(error);
