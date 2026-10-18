/**
 * The primary workflow: uninstall Acme Widget with a Moderate scan, remove
 * the leftovers, and check the fake machine's files and registry changed.
 */
import { strict as assert } from "node:assert";
import fs from "node:fs";

import { dataDir, machinePath } from "./workspace.ts";
import {
  clearSearch,
  dialogButton,
  leftoverPaths,
  programNames,
  ready,
  registryKeys,
  screenshot,
  selectProgram,
  waitForButton,
} from "./support.ts";

describe("uninstalling a program", () => {
  it("runs its uninstaller, then finds what it left behind", async () => {
    await ready();
    await selectProgram("Acme Widget");
    await $("#un-uninstall").click();
    await $("#un-wizard-start").waitForDisplayed({ timeout: 10_000 });
    assert.ok(
      await $("#un-mode-moderate").isSelected(),
      "Moderate is the default",
    );
    assert.ok(await $("#un-wizard-restore").isSelected());
    await screenshot("wizard-confirm");
    await $("#un-wizard-start").click();

    const paths = await leftoverPaths();
    const has = (ending: string) => paths.some((p) => p.endsWith(ending));
    for (const ending of [
      "Program Files\\Acme\\Widget",
      "AppData\\Roaming\\Acme\\Widget",
      "HKLM\\SOFTWARE\\Acme\\Widget",
      "App Paths\\widget.exe",
      "Services\\AcmeWidgetSvc",
    ]) {
      assert.ok(has(ending), `missing ${ending} in ${paths.join("\n")}`);
    }
    assert.ok(!paths.some((p) => p.includes("Acme Widget Helper")));
    assert.ok(!paths.some((p) => p.endsWith("Program Files\\Acme")));
    const steps = await $("#un-leftover-summary").getText();
    assert.match(
      steps,
      new RegExp(`^[0-9]+ of ${paths.length} items selected`),
    );
    await screenshot("wizard-leftovers");
  });

  it("removes the ticked leftovers with a backup", async () => {
    // Keep one item to prove only ticked items go.
    const kept = await $('.leftover input[type="checkbox"]');
    await kept.click();
    assert.equal(await kept.isSelected(), false);
    await (await waitForButton("wizard-remove")).click();
    await dialogButton("Remove");
    await $("#un-removal-summary").waitForDisplayed({ timeout: 60_000 });
    assert.match(
      await $("#un-removal-summary").getText(),
      /^Removed .+ freeing /,
    );
    await screenshot("wizard-done");
    await (await waitForButton("wizard-close")).click();

    assert.ok(!fs.existsSync(machinePath("Program Files\\Acme\\Widget")));
    assert.ok(fs.existsSync(machinePath("Program Files\\Acme\\Helper")));
    const keys = registryKeys();
    assert.ok(!keys.includes("HKLM\\software\\acme\\widget"));
    // The unticked first item, HKCU\Software\Acme\Widget, is still there.
    assert.ok(keys.includes("HKCU\\software\\acme\\widget"));
    assert.ok(keys.includes("HKLM\\software\\acme\\helper"));
    const backups = fs.readdirSync(`${dataDir()}\\backups`);
    assert.equal(backups.length, 1, "one backup folder was written");
  });

  it("drops the program from the list", async () => {
    await clearSearch();
    await browser.waitUntil(async () => (await programNames()).length === 5, {
      timeout: 10_000,
      timeoutMsg: "the list did not refresh after the uninstall",
    });
    assert.ok(!(await programNames()).includes("Acme Widget 2.1 (x64)"));
  });
});
