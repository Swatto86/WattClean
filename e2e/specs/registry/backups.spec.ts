/** The Backup Center restores chosen entries, exports and backs up by hand. */
import { strict as assert } from "node:assert";
import fs from "node:fs";
import path from "node:path";

import { dataDir } from "./workspace.ts";
import {
  chooseFromMenu,
  clickTab,
  dialogButton,
  dialogText,
  dialogTitled,
  machine,
  registryValue,
  screenshot,
} from "./support.ts";

const RUN = String.raw`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`;

describe("the Backup Center", () => {
  it("lists the clean's backup", async () => {
    await clickTab("backups");
    await $("#reg-backup-list article").waitForDisplayed({ timeout: 10_000 });
    const text = await $("#reg-backup-list article").getText();
    assert.match(text, /Registry clean, 8 entries/);
    assert.match(text, /8 removed/);
    await screenshot("backups");
  });

  it("restores one chosen entry from the native menu", async () => {
    await chooseFromMenu("#reg-backup-list article", "Choose what to restore");
    await $(".restore-item").waitForDisplayed({ timeout: 10_000 });
    let ticked = false;
    for (const label of await $$(".restore-item")) {
      if ((await label.getText()).includes(`${RUN} | FabrikamUpdater`)) {
        await label.$("input").click();
        ticked = true;
      }
    }
    assert.ok(ticked, "the Run value was not listed");
    await dialogButton("Restore ticked");
    assert.match(await dialogText(), /1 entry restored\./);
    await dialogButton("OK");
    assert.ok(registryValue(RUN, "FabrikamUpdater"), "it was not put back");
  });

  it("exports the backup as a .reg file Registry Editor reads", async () => {
    await $("#reg-backup-list article").$("button=Export .reg…").click();
    assert.match(await dialogText(), /Saved to/);
    await dialogButton("OK");
    const folder = path.join(dataDir(), "exports");
    const file = fs.readdirSync(folder).find((name) => name.endsWith(".reg"));
    assert.ok(file, "no .reg file was written");
    const bytes = fs.readFileSync(path.join(folder, file));
    assert.deepEqual([...bytes.subarray(0, 2)], [0xff, 0xfe], "UTF-16 mark");
    const text = bytes.subarray(2).toString("utf16le");
    assert.ok(text.startsWith("Windows Registry Editor Version 5.00\r\n"));
    assert.match(text, /"FabrikamUpdater"=/);
  });

  it("backs up a key by hand", async () => {
    await $("#reg-backup-key").click();
    await $("#reg-backup-key-path").setValue(
      String.raw`HKEY_CURRENT_USER\Software\Northwind`,
    );
    await $("#reg-backup-key-label").setValue("Northwind before tidying");
    await dialogButton("Back up");
    await browser.waitUntil(
      async () => (await $$("#reg-backup-list article").length) === 2,
      { timeout: 10_000, timeoutMsg: "the hand backup was not listed" },
    );
    assert.match(
      await $("#reg-backup-list article").getText(),
      /Northwind before tidying\s+Hand backup/,
    );
  });

  it("backs up the whole registry, which is not put back from here", async () => {
    await $("#reg-backup-full").click();
    assert.match(await dialogTitled("Full backup made"), /5 hives saved/);
    await dialogButton("OK");
    const full = await $("#reg-backup-list article");
    await browser.waitUntil(async () =>
      /Full registry backup\s+Full registry/.test(await full.getText()),
    );
    assert.equal(
      await full.$("button=Restore").isExisting(),
      false,
      "a full backup offers no restore",
    );
    // Leave the list as the later specs expect it.
    await $("#reg-backup-list article").$("button=Delete").click();
    await dialogButton("Delete");
    await browser.waitUntil(
      async () => (await $$("#reg-backup-list article").length) === 2,
    );
  });
});
