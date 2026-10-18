/** Shared steps for the specs. Not named `*.spec.ts`, so it opens no session. */
import { strict as assert } from "node:assert";
import fs from "node:fs";
import path from "node:path";

import { application } from "../../wdio.conf.ts";
import { dataDir, stopApp } from "./workspace.ts";

/**
 * The program list is showing and loaded. The spec files share one session,
 * so each starts from wherever the previous one left the app.
 */
export async function ready(): Promise<void> {
  await $("#nav-programs").waitForExist({ timeout: 30_000 });
  await $("#nav-programs").click();
  await browser.waitUntil(
    async () => /\d+ programs/.test(await $("#un-program-count").getText()),
    { timeout: 30_000, timeoutMsg: "the program list never loaded" },
  );
}

/** The suite's page for each of the uninstaller's views. */
const PAGES: Record<string, string> = {
  programs: "programs",
  traced: "traced",
  forced: "forced",
  backups: "unbackups",
  history: "unhistory",
  settings: "settings",
};

/** Opens the page showing one of the uninstaller's views. */
export async function clickTab(view: string): Promise<void> {
  await $(`#nav-${PAGES[view] ?? view}`).click();
  await $(`#un-view-${view}`).waitForDisplayed({ timeout: 5_000 });
}

export async function programNames(): Promise<string[]> {
  const rows = await $$("#un-program-rows tr");
  const names: string[] = [];
  for (const row of rows) {
    names.push(await row.$("td .name-cell span:nth-child(2)").getText());
  }
  return names;
}

export async function selectProgram(name: string): Promise<void> {
  await $("#un-search").setValue(name);
  const row = await $(`#un-program-rows tr[data-id*="${idPart(name)}"]`);
  await row.waitForExist({ timeout: 5_000 });
  await row.click();
  assert.equal(await row.getAttribute("aria-selected"), "true");
}

/** The uninstall key name each seeded program is registered under. */
function idPart(name: string): string {
  const keys: Record<string, string> = {
    "Acme Widget": "AcmeWidget",
    "Northwind Broken Tool": "BrokenTool",
    "Contoso Notes": "ContosoNotes",
    "Fabrikam Viewer": "0A1B2C3D",
    "Tailspin Paint": "TailspinPaint",
  };
  const key = keys[name];
  if (!key) throw new Error(`no seeded program called ${name}`);
  return key;
}

export async function waitForButton(id: string, timeout = 30_000) {
  const button = await $(`#un-${id}`);
  await button.waitForClickable({ timeout });
  return button;
}

/** Click the dialog button with this label in the topmost dialog. */
export async function dialogButton(label: string): Promise<void> {
  const top = await $("#un-dialog-root > .backdrop:last-child .dialog");
  assert.ok(await top.isExisting(), "no dialog is open");
  const buttons = await top.$$("footer button");
  for (const button of buttons) {
    if ((await button.getText()) === label) {
      await button.click();
      return;
    }
  }
  throw new Error(`no "${label}" button in the dialog`);
}

export async function leftoverPaths(): Promise<string[]> {
  await $("#un-leftover-summary").waitForExist({ timeout: 60_000 });
  const paths: string[] = [];
  for (const item of await $$(".leftover .path"))
    paths.push(await item.getText());
  return paths;
}

export function screenshotFolder(): string {
  const folder = path.join(dataDir(), "screens");
  fs.mkdirSync(folder, { recursive: true });
  return folder;
}

export async function screenshot(name: string): Promise<void> {
  await browser.saveScreenshot(path.join(screenshotFolder(), `${name}.png`));
}

/** Stop the app and start it again in a new session: a restart. */
export async function restartApp(): Promise<void> {
  await stopApp(application);
  await browser.reloadSession();
  await ready();
}

/** Empty the search box with the keyboard, as a person would. */
export async function clearSearch(): Promise<void> {
  await $("#un-search").click();
  await browser.keys(["Control", "a"]);
  await browser.keys(["Backspace"]);
}

/** Folded registry key paths of the fake machine, e.g. `HKLM\software\acme`. */
export function registryKeys(): string[] {
  const saved: unknown = JSON.parse(
    fs.readFileSync(path.join(dataDir(), "machine", "registry.json"), "utf8"),
  );
  if (typeof saved !== "object" || saved === null || !("keys" in saved)) {
    throw new Error("registry.json has no keys");
  }
  const keys = saved.keys;
  if (typeof keys !== "object" || keys === null) throw new Error("bad keys");
  return Object.keys(keys);
}
