/**
 * Install with tracing: the log records what the installer made, the program
 * shows as traced, and its uninstall removes what only the log knew about
 * while never touching the PATH value the installer changed.
 */
import { strict as assert } from "node:assert";
import fs from "node:fs";

import { machinePath, readJson } from "./workspace.ts";
import {
  clickTab,
  dialogButton,
  leftoverPaths,
  ready,
  selectProgram,
  waitForButton,
} from "./support.ts";

describe("installing with tracing", () => {
  it("records the install in a log", async () => {
    await ready();
    await clickTab("traced");
    assert.match(await $("#un-log-list").getText(), /No traced installs yet/);
    await $("#un-trace-install").click();
    await $("#un-trace-result").waitForDisplayed({ timeout: 30_000 });
    assert.match(
      await $("#un-trace-result").getText(),
      /^Recorded \d+ items that Tailspin Paint created, and 1 existing item it changed\.$/,
    );
    await (await waitForButton("trace-close")).click();
    const card = await $("article[data-log]");
    await card.waitForDisplayed({ timeout: 10_000 });
    assert.match(await card.getText(), /Tailspin Paint/);
    assert.ok(fs.existsSync(machinePath("ProgramData\\TSXData\\brushes.db")));
  });

  it("shows what was created and what was only changed", async () => {
    await $('article[data-log] button[data-action="view"]').click();
    await $(".dialog .leftover").waitForDisplayed({ timeout: 10_000 });
    const text = await $(".dialog .body").getText();
    assert.match(text, /ProgramData\\TSXData\nCreated/);
    assert.match(text, /Environment → Path\nChanged \(kept\)/);
    assert.doesNotMatch(text, /tsx_setup\.tmp|RestartManager/);
    await dialogButton("Close");
  });

  it("marks the program as traced and uninstalls by its log", async () => {
    await ready();
    await selectProgram("Tailspin Paint");
    const row = await $('#un-program-rows tr[aria-selected="true"]');
    assert.match(await row.getText(), /Traced/);
    await $("#un-uninstall").click();
    await $("#un-wizard-logged").waitForDisplayed({ timeout: 10_000 });
    await $("#un-mode-safe").click();
    await $("#un-wizard-restore").click();
    await $("#un-wizard-start").click();
    const paths = await leftoverPaths();
    assert.ok(
      paths.some((p) => p.endsWith("ProgramData\\TSXData")),
      paths.join("\n"),
    );
    assert.ok(paths.some((p) => p.endsWith("TSXUpdater")));
    assert.ok(!paths.some((p) => p.includes("Environment")));
    await (await waitForButton("wizard-remove")).click();
    await dialogButton("Remove");
    await $("#un-removal-summary").waitForDisplayed({ timeout: 60_000 });
    await (await waitForButton("wizard-close")).click();
    assert.ok(!fs.existsSync(machinePath("ProgramData\\TSXData")));
    const registry = JSON.stringify(readJson("machine/registry.json"));
    assert.match(registry, /Session Manager\\\\Environment/);
  });
});
