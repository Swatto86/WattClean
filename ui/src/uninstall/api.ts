/** Typed wrappers for every Rust command; results are checked on arrival. */
import { invoke } from "@tauri-apps/api/core";

import { bool, list, optStr, str } from "./decode.ts";
import {
  appInfo,
  backupSummary,
  historyEntry,
  nothing,
  program,
  removal,
  report,
  restoreView,
  sessionView,
  settings,
  type Settings,
  type Mode,
} from "./schema.ts";
import { installLog, logSummary, traceStarted } from "./schema-logs.ts";

export * from "./schema.ts";
export * from "./schema-logs.ts";

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
  getSettings: () => call("un_get_settings", {}, settings),
  saveSettings: (value: Settings) =>
    call("un_save_settings", { settings: value }, nothing),
  listInstalled: () =>
    call("un_list_installed", {}, (v, w) => list(v, w, program)),
  beginUninstall: (programId: string) =>
    call("un_begin_uninstall", { programId }, sessionView),
  beginForced: (name: string, folder: string) =>
    call("un_begin_forced", { name, folder }, sessionView),
  /** One restore point before `batch` programs, this session's first. */
  createRestorePoint: (session: number, batch: number) =>
    call("un_create_restore_point", { session, batch }, nothing),
  runUninstaller: (session: number, quiet: boolean) =>
    call("un_run_uninstaller", { session, quiet }, str),
  uninstallerFinished: (session: number) =>
    call("un_uninstaller_finished", { session }, bool),
  scanLeftovers: (session: number, mode: Mode) =>
    call("un_scan_leftovers", { session, mode }, report),
  scanProgress: (session: number) => call("un_scan_progress", { session }, str),
  cancelScan: (session: number) => call("un_cancel_scan", { session }, nothing),
  removeLeftovers: (session: number, ids: number[]) =>
    call("un_remove_leftovers", { session, ids }, removal),
  endUninstall: (session: number) =>
    call("un_end_uninstall", { session }, nothing),
  listBackups: () =>
    call("un_list_backups", {}, (v, w) => list(v, w, backupSummary)),
  restoreBackup: (id: string) => call("un_restore_backup", { id }, restoreView),
  deleteBackup: (id: string) => call("un_delete_backup", { id }, nothing),
  listHistory: () =>
    call("un_list_history", {}, (v, w) => list(v, w, historyEntry)),
  clearHistory: () => call("un_clear_history", {}, nothing),
  restartAsAdmin: () => call("restart_as_admin", {}, nothing),
  pickInstaller: () => call("un_pick_installer", {}, optStr),
  beginTrace: (path: string) => call("un_begin_trace", { path }, traceStarted),
  traceFinished: (id: number) => call("un_trace_finished", { id }, bool),
  finishTrace: (id: number) => call("un_finish_trace", { id }, logSummary),
  listLogs: () => call("un_list_logs", {}, (v, w) => list(v, w, logSummary)),
  getLog: (id: string) => call("un_get_log", { id }, installLog),
  deleteLog: (id: string) => call("un_delete_log", { id }, nothing),
};
