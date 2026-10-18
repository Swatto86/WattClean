/** Restoring the Acme Widget removal puts its files and keys back. */
import { strict as assert } from "node:assert";
import fs from "node:fs";

import { machinePath, readJson } from "./workspace.ts";
import { clickTab, dialogButton, ready, screenshot } from "./support.ts";

describe("backups", () => {
  it("lists the removal and restores it", async () => {
    await ready();
    await clickTab("backups");
    const card = await $("article[data-backup]");
    await card.waitForDisplayed({ timeout: 10_000 });
    assert.match(await card.getText(), /Acme Widget 2\.1 \(x64\)/);
    assert.match(await card.getText(), /Can be restored/);
    await screenshot("backups");

    await card.$('button[data-action="restore"]').click();
    await dialogButton("Restore");
    await browser.waitUntil(
      async () => (await $(".dialog h2").getText()) === "Restore finished",
      { timeout: 30_000 },
    );
    assert.match(await $(".dialog .body").getText(), /^Restored \d+ items\.$/);
    await dialogButton("OK");

    assert.ok(
      fs.existsSync(machinePath("Program Files\\Acme\\Widget\\settings.ini")),
    );
    const registry = JSON.stringify(
      readJson("machine/registry.json"),
    ).toLowerCase();
    assert.ok(registry.includes("software\\\\acme\\\\widget"));
    const restored = await $("article[data-backup]");
    await browser.waitUntil(async () =>
      /Restored/.test(await restored.getText()),
    );
    assert.ok(
      (await restored
        .$('button[data-action="restore"]')
        .getAttribute("disabled")) !== null,
      "a backup restores once",
    );
  });
});
