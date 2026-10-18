/** Settings, history and the machine's state survive a restart. */
import { strict as assert } from "node:assert";

import { readJson } from "./workspace.ts";
import {
  clickTab,
  programNames,
  ready,
  restartApp,
  screenshot,
} from "./support.ts";

describe("persistence across a restart", () => {
  it("saves a setting at once", async () => {
    await ready();
    await clickTab("settings");
    await $("#un-set-mode").selectByAttribute("value", "advanced");
    await browser.waitUntil(
      async () =>
        /"mode":"advanced"/.test(JSON.stringify(readJson("settings.json"))),
      { timeout: 5_000, timeoutMsg: "the mode was not saved" },
    );
    const saved = readJson("settings.json");
    assert.ok(typeof saved === "object" && saved !== null);
    assert.ok("mode" in saved && saved.mode === "advanced");
  });

  it("comes back with the same settings and machine state", async () => {
    await restartApp();
    await clickTab("settings");
    assert.equal(await $("#un-set-mode").getValue(), "advanced");
    await clickTab("programs");
    // Acme Widget was restored from backup, but its uninstall entry was
    // removed by its uninstaller, so it stays off the list.
    const names = await programNames();
    assert.ok(!names.includes("Acme Widget 2.1 (x64)"), names.join(", "));
    assert.ok(names.includes("Acme Widget Helper"));
    await screenshot("uninstall-persisted");
    await clickTab("history");
    await $("article[data-history]").waitForDisplayed({ timeout: 10_000 });
    assert.equal((await $$("article[data-history]")).length, 3);
  });
});
