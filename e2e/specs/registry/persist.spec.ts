/** Settings, the watch list and tweaks survive a restart. */
import { strict as assert } from "node:assert";
import fs from "node:fs";
import path from "node:path";

import { dataDir, readJson } from "./workspace.ts";
import {
  clickTab,
  dialogButton,
  dialogTitled,
  restartApp,
  screenshot,
} from "./support.ts";

const KEEP = String.raw`HKEY_CURRENT_USER\Software\Keep Me`;

describe("persistence across a restart", () => {
  it("saves settings and list entries at once", async () => {
    await clickTab("settings");
    await $("#reg-set-keep-days").setValue("30");
    await browser.keys(["Tab"]);
    await browser.waitUntil(
      async () =>
        /"keepBackupsDays":30/.test(JSON.stringify(readJson("settings.json"))),
      { timeout: 5_000, timeoutMsg: "the keeping time was not saved" },
    );
    await $("#reg-set-keep-mb").setValue("500");
    await browser.keys(["Tab"]);
    await browser.waitUntil(
      async () =>
        /"keepBackupsMb":500/.test(JSON.stringify(readJson("settings.json"))),
      { timeout: 5_000, timeoutMsg: "the size limit was not saved" },
    );
    const input = await $("#reg-excludes input:not([type])");
    await input.setValue(KEEP);
    await $("#reg-excludes .key-add button").click();
    await $(
      `#reg-excludes .key-row[data-key="${KEEP.replaceAll("\\", "\\\\")}"]`,
    ).waitForExist({
      timeout: 5_000,
    });
    assert.match(JSON.stringify(readJson("settings.json")), /Keep Me/);
  });

  it("refuses a key that is not one", async () => {
    await $("#reg-includes input:not([type])").setValue(String.raw`HKCR\.txt`);
    await $("#reg-includes .key-add button").click();
    await browser.waitUntil(
      async () =>
        /HKEY_CLASSES_ROOT combines two keys/.test(
          await $("#reg-includes .error").getText(),
        ),
      { timeoutMsg: "no error for HKCR" },
    );
  });

  it("exports the settings and imports them back", async () => {
    await $("#reg-settings-export").click();
    await browser.waitUntil(async () =>
      /Settings saved to/.test(await $("#reg-settings-status").getText()),
    );
    const file = path.join(
      dataDir(),
      "exports",
      "WattClean registry settings.json",
    );
    assert.match(fs.readFileSync(file, "utf8"), /"keepBackupsDays": 30/);
    await $("#reg-set-keep-days").setValue("7");
    await browser.keys(["Tab"]);
    await browser.waitUntil(
      async () =>
        /"keepBackupsDays":7/.test(JSON.stringify(readJson("settings.json"))),
      { timeout: 5_000, timeoutMsg: "the keeping time was not saved" },
    );
    await $("#reg-settings-import").click();
    await browser.waitUntil(async () =>
      /Settings imported/.test(await $("#reg-settings-status").getText()),
    );
    assert.equal(await $("#reg-set-keep-days").getValue(), "30");
  });

  it("moves the backups to another folder and back", async () => {
    const home = path.join(dataDir(), "backups");
    const moved = path.join(dataDir(), "moved-backups");
    const count = fs.readdirSync(home).length;
    assert.ok(count > 0, "earlier specs left no backups to move");
    await $("#reg-backups-change").click();
    assert.match(await dialogTitled("Backups moved"), /moved-backups/);
    await dialogButton("OK");
    assert.equal(fs.readdirSync(moved).length, count, "not all were moved");
    assert.match(await $("#reg-backups-folder").getText(), /moved-backups/);
    await clickTab("backups");
    await browser.waitUntil(
      async () => (await $$("#reg-backup-list article").length) === count,
      { timeout: 10_000, timeoutMsg: "the moved backups are not listed" },
    );
    await clickTab("settings");
    await $("#reg-backups-default").click();
    await dialogTitled("Backups moved");
    await dialogButton("OK");
    assert.equal(fs.readdirSync(moved).length, 0, "they did not come back");
    assert.equal(fs.readdirSync(home).length, count);
  });

  it("comes back with the same settings, watch and tweak", async () => {
    await restartApp();
    await clickTab("settings");
    assert.match(await $("#reg-excludes").getText(), /Keep Me/);
    await clickTab("watcher");
    await $("#reg-watch-list .watch-item").waitForDisplayed({
      timeout: 10_000,
    });
    assert.match(await $("#reg-watch-list .watch-item").getText(), /Watching/);
    await browser.waitUntil(
      async () =>
        (await $("#reg-watch-events").getText()).includes(
          "ShowSecondsInSystemClock",
        ),
      { timeout: 10_000, timeoutMsg: "the logged change did not survive" },
    );
    assert.match(await $("#reg-watch-events").getText(), /wattclean\.exe/);
    await clickTab("tuner");
    await $("#reg-tweak-secondsInClock").waitForExist({ timeout: 10_000 });
    assert.ok(await $("#reg-tweak-secondsInClock").isSelected());
    await screenshot("registry-persisted");
  });
});
