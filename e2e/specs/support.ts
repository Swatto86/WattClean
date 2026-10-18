/** Shared steps for the specs. Not named `*.spec.ts`, so it opens no session. */
import fs from "node:fs";
import path from "node:path";

import { application } from "../wdio.conf.ts";
import { dataDir, stopApp } from "../workspace.ts";

/** The app has booted and listed its cleaners. The spec files share one
 * session, so each starts from wherever the previous one left the app. */
export async function ready(): Promise<void> {
  await $("#nav-cleaner").waitForExist({ timeout: 30_000 });
  await browser.waitUntil(
    async () => (await $$("#cleaner-tree .cleaner-row").length) > 0,
    { timeout: 30_000, timeoutMsg: "the cleaner list never appeared" },
  );
}

export async function openPage(page: string): Promise<void> {
  await $(`#nav-${page}`).click();
  await $(`#page-${page}`).waitForDisplayed({ timeout: 5_000 });
}

/** Click the button with this label in the open dialog. */
export async function dialogButton(label: string): Promise<void> {
  const dialog = await $("dialog.dialog[open]");
  await dialog.waitForExist({ timeout: 15_000 });
  for (const button of await dialog.$$(".dialog-buttons button")) {
    if ((await button.getText()) === label) {
      await button.click();
      await browser.waitUntil(
        async () => !(await $("dialog.dialog[open]").isExisting()),
        { timeout: 5_000, timeoutMsg: "the dialog did not close" },
      );
      return;
    }
  }
  throw new Error(`no "${label}" button in the dialog`);
}

/** The open dialog's title and text. */
export async function dialogText(): Promise<string> {
  const dialog = await $("dialog.dialog[open]");
  await dialog.waitForExist({ timeout: 15_000 });
  return dialog.getText();
}

export async function screenshot(name: string): Promise<void> {
  const folder =
    process.env["WATTCOMPCLEAN_E2E_SHOTS"] ?? path.join(dataDir(), "screens");
  fs.mkdirSync(folder, { recursive: true });
  await browser.saveScreenshot(path.join(folder, `${name}.png`));
}

/** Stop the app and start it again in a new session: a restart. */
export async function restartApp(): Promise<void> {
  await stopApp(application);
  await browser.reloadSession();
  await ready();
}

/** Ticks exactly these options (`cleaner.option`) through the tree, on the
 * Disk cleaner page. */
export async function tickOnly(keys: string[]): Promise<void> {
  if (!(await $("#page-cleaner").isDisplayed())) await openPage("cleaner");
  await $("#select-none").click();
  for (const key of keys) {
    const [cleaner = "", option = ""] = key.split(".");
    const box = await $(`#opt-${cleaner}-${option}`);
    if (!(await box.isDisplayed())) {
      await $(`.cleaner-row[data-cleaner="${cleaner}"] .expander`).click();
    }
    await box.waitForDisplayed({ timeout: 5_000 });
    if (!(await box.isSelected())) await box.click();
  }
}

/** The test machine's state as the app last saved it. */
export function machineState(): {
  registry: Record<string, string[]>;
  clipboard: boolean;
  running: string[];
  reboot_deletions: string[];
  shell_notifications: number;
} {
  return JSON.parse(
    fs.readFileSync(
      path.join(dataDir(), "disk", "machine", "state.json"),
      "utf8",
    ),
  );
}
