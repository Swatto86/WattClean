/** Shared steps for the specs. Not named `*.spec.ts`, so it opens no session. */
import { strict as assert } from "node:assert";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { application } from "../../wdio.conf.ts";
import { appPids, dataDir, stopApp } from "./workspace.ts";

const helper = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../native-menu.ps1",
);

import { ready } from "../support.ts";

export { ready };

/** The suite's page for each of the registry cleaner's views. */
const PAGES: Record<string, string> = {
  cleaner: "registry",
  search: "regsearch",
  tuner: "tuner",
  watcher: "watcher",
  backups: "regbackups",
  schedules: "schedules",
  settings: "settings",
};

/** Opens the page showing one of the registry cleaner's views. */
export async function clickTab(view: string): Promise<void> {
  await $(`#nav-${PAGES[view] ?? view}`).click();
  await $(`#reg-view-${view}`).waitForDisplayed({ timeout: 5_000 });
}

/** Click the button with this label in the topmost dialog. */
export async function dialogButton(label: string): Promise<void> {
  const top = await $("#reg-dialog-root > .backdrop:last-child .dialog");
  await top.waitForExist({ timeout: 30_000 });
  for (const button of await top.$$("footer button")) {
    if ((await button.getText()) === label) {
      await button.click();
      return;
    }
  }
  throw new Error(`no "${label}" button in the dialog`);
}

/** The text of the dialog titled `title`, once it is the topmost one. */
export async function dialogTitled(title: string): Promise<string> {
  await browser.waitUntil(
    async () => {
      const top = await $("#reg-dialog-root > .backdrop:last-child .dialog h2");
      return (await top.isExisting()) && (await top.getText()) === title;
    },
    { timeout: 30_000, timeoutMsg: `no "${title}" dialog appeared` },
  );
  return $("#reg-dialog-root > .backdrop:last-child .dialog .body").getText();
}

/** The topmost dialog's text, once one is open. */
export async function dialogText(): Promise<string> {
  const top = await $("#reg-dialog-root > .backdrop:last-child .dialog .body");
  await top.waitForExist({ timeout: 30_000 });
  return top.getText();
}

export async function screenshot(name: string): Promise<void> {
  const folder = path.join(dataDir(), "screens");
  fs.mkdirSync(folder, { recursive: true });
  await browser.saveScreenshot(path.join(folder, `${name}.png`));
}

/** Stop the app and start it again in a new session: a restart. */
export async function restartApp(): Promise<void> {
  await stopApp(application);
  await browser.reloadSession();
  await ready();
}

interface SavedValue {
  name: string;
  kind: number;
  bytes: string;
}

/** The fake machine as the app last saved it. */
export function machine(): {
  keys: Record<string, { values: SavedValue[]; security?: string | null }>;
  opened: string[];
  opened_urls?: string[];
  tasks?: Record<string, string>;
  /** Title and text of each notification shown. */
  notifications?: [string, string][];
  restore_points: string[];
  explorer_restarts: number;
  sign_outs?: number;
  pc_restarts?: number;
} {
  const text = fs.readFileSync(
    path.join(dataDir(), "machine", "registry.json"),
    "utf8",
  );
  return JSON.parse(text);
}

/** A key as the fake machine files it: hive as written, path lower-case. */
function folded(key: string): string {
  const [hive = "", ...rest] = key.split("\\");
  return [hive.toUpperCase(), ...rest.map((part) => part.toLowerCase())].join(
    "\\",
  );
}

/** A value of the fake registry, by key (`HKCU\…`) and name; or undefined. */
export function registryValue(
  key: string,
  name: string,
): SavedValue | undefined {
  const entry = machine().keys[folded(key)];
  return entry?.values.find((v) => v.name.toLowerCase() === name.toLowerCase());
}

/** A key's owner and permissions in the fake registry (SDDL), if set. */
export function keySecurity(key: string): string | null {
  return machine().keys[folded(key)]?.security ?? null;
}

export function hasKey(key: string): boolean {
  return folded(key) in machine().keys;
}

/** Wait until the fake machine shows what `check` looks for. */
export async function untilMachine(
  check: () => boolean,
  what: string,
): Promise<void> {
  await browser.waitUntil(async () => check(), {
    timeout: 15_000,
    timeoutMsg: `the machine never showed ${what}`,
  });
}

/**
 * Right-click an element as a person would and say which menu the page let
 * through: "none" when it kept the browser's menu away, "browser" when not.
 */
export async function rightClick(
  selector: string,
): Promise<string | undefined> {
  await browser.execute(() => {
    delete document.documentElement.dataset["menu"];
    window.addEventListener(
      "contextmenu",
      (event) => {
        document.documentElement.dataset["menu"] = event.defaultPrevented
          ? "none"
          : "browser";
      },
      { once: true },
    );
  });
  await $(selector).click({ button: "right" });
  return browser.execute(() => document.documentElement.dataset["menu"]);
}

/**
 * The Windows menu the app has open, read through Windows itself: "Cut
 * Ctrl+X" per enabled item, "(Cut Ctrl+X)" per greyed one, "-" per
 * separator, [] when none opens within `waitMs`. With `choose`, the item
 * whose label starts with it is chosen; otherwise the menu is closed.
 */
export function nativeMenu(waitMs = 5_000, choose = ""): string[] {
  const args = [
    "-NoProfile",
    "-NonInteractive",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    helper,
    "-ProcessIds",
    appPids(application).join(","),
    "-WaitMs",
    String(waitMs),
  ];
  if (choose) args.push("-Choose", choose);
  const output = execFileSync("powershell.exe", args, { encoding: "utf8" });
  const items: unknown = JSON.parse(output.trim().split(/\r?\n/)[0] ?? "[]");
  assert.ok(
    Array.isArray(items) && items.every((item) => typeof item === "string"),
    output,
  );
  return items;
}

/** Right-click, then choose an item of the native menu that opens. */
export async function chooseFromMenu(
  selector: string,
  label: string,
): Promise<string[]> {
  assert.equal(await rightClick(selector), "none", selector);
  return nativeMenu(5_000, label);
}

/** The summary line above a findings tree. */
export async function summaryOf(view: "cleaner" | "search"): Promise<string> {
  return $(`#reg-${view}-summary`).getText();
}
