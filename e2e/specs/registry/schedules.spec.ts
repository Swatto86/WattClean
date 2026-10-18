/**
 * Schedules are kept, refused by Task Scheduler from a development build
 * (Windows would start a file that moves), run on demand and headless
 * (ending with a notification), paused and deleted from the native menu.
 */
import { strict as assert } from "node:assert";
import { execFileSync } from "node:child_process";

import { application } from "../../wdio.conf.ts";
import { readJson } from "./workspace.ts";
import {
  chooseFromMenu,
  clickTab,
  dialogButton,
  dialogTitled,
  machine,
  screenshot,
} from "./support.ts";

describe("schedules", () => {
  it("saves an automatic backup but does not schedule a dev build", async () => {
    await clickTab("schedules");
    await $("#reg-job-new").click();
    await $("#reg-job-name").waitForDisplayed({ timeout: 10_000 });
    await $("#reg-job-name").setValue("Back up Northwind");
    await $("#reg-job-kind").selectByAttribute("value", "backup");
    await $("#reg-job-key").setValue(
      String.raw`HKEY_CURRENT_USER\Software\Northwind`,
    );
    await $("#reg-job-every").selectByAttribute("value", "day");
    await dialogButton("Save");
    assert.match(
      await dialogTitled("Saved, but not scheduled"),
      /Not scheduled: this is a development build/,
    );
    await dialogButton("OK");
    const card = await $("#reg-job-list article");
    await card.waitForDisplayed({ timeout: 10_000 });
    assert.match(await card.getText(), /Back up Northwind\s+Not scheduled/);
    assert.match(await card.getText(), /Every day at 03:00/);
    assert.deepEqual(Object.keys(machine().tasks ?? {}), []);
    await screenshot("schedules");
  });

  it("keeps the editor open when the schedule is refused", async () => {
    await $("#reg-job-new").click();
    await $("#reg-job-kind").waitForDisplayed({ timeout: 10_000 });
    await $("#reg-job-kind").selectByAttribute("value", "backup");
    await $("#reg-job-key").setValue(String.raw`HKCR\.txt`);
    await dialogButton("Save");
    await browser.waitUntil(
      async () => /HKEY_CLASSES_ROOT/.test(await $(".dialog .error").getText()),
      { timeoutMsg: "no reason shown" },
    );
    await dialogButton("Cancel");
  });

  it("runs on demand", async () => {
    await $("#reg-job-list article").$("button=Run now").click();
    assert.match(
      await dialogTitled('"Back up Northwind" ran'),
      /Backed up HKCU\\Software\\Northwind\./,
    );
    await dialogButton("OK");
    await browser.waitUntil(async () =>
      /Last run .*Backed up/.test(await $("#reg-job-list article").getText()),
    );
    await clickTab("backups");
    await browser.waitUntil(async () =>
      (await $("#reg-backup-list").getText()).includes(
        "Auto backup: Back up Northwind",
      ),
    );
    await clickTab("schedules");
  });

  it("runs headless as Task Scheduler would, then notifies", () => {
    const id = /"id":"([^"]+)"/.exec(
      JSON.stringify(readJson("jobs.json")),
    )?.[1];
    assert.ok(id, "no saved job");
    // Throws unless the run reports success (exit code 0).
    execFileSync(application, ["--run-job", id], {
      env: process.env,
      stdio: "pipe",
      timeout: 60_000,
    });
    assert.deepEqual(machine().notifications?.at(-1), [
      '"Back up Northwind" ran',
      String.raw`Backed up HKCU\Software\Northwind.`,
    ]);
  });

  it("pauses and deletes from the native menu", async () => {
    await chooseFromMenu("#reg-job-list article", "Pause");
    await browser.waitUntil(async () =>
      /Paused/.test(await $("#reg-job-list article").getText()),
    );
    await chooseFromMenu("#reg-job-list article", "Delete");
    await dialogButton("Delete");
    await $("#reg-job-list .empty").waitForDisplayed({ timeout: 10_000 });
    const saved = readJson("jobs.json");
    assert.match(JSON.stringify(saved), /"jobs":\[\]/);
  });
});
