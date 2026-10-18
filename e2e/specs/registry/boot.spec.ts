/** Boot to the main window and run the primary workflow's scan. */
import { strict as assert } from "node:assert";

import { clickTab, ready, screenshot, summaryOf } from "./support.ts";

describe("WattClean boots", () => {
  it("opens the Registry Cleaner for the test machine", async () => {
    await ready();
    await clickTab("cleaner");
    assert.ok(await $("#reg-view-cleaner").isDisplayed());
    assert.ok(await $("#test-machine").isDisplayed(), "test build is labelled");
    assert.equal(await $("#status-rights").getText(), "Administrator");
    assert.ok(await $('input[name="mode"][value="safe"]').isSelected());
    await screenshot("start");
  });

  it("finds the test machine's broken entries with a Safe scan", async () => {
    await $("#reg-cleaner-scan").click();
    await $("#reg-cleaner-results").waitForDisplayed({ timeout: 60_000 });
    assert.equal(await summaryOf("cleaner"), "8 entries found, 8 ticked.");
    const groups: string[] = [];
    for (const group of await $$("#reg-cleaner-tree .group-row strong")) {
      groups.push(await group.getText());
    }
    assert.deepEqual(groups, [
      "Missing startup items",
      "Wrong application paths",
      "Invalid cache entries",
      "Missing shared DLLs",
      "Incorrect uninstaller data",
      "Broken context menu commands",
    ]);
    const paths: string[] = [];
    for (const path of await $$("#reg-cleaner-tree .finding .path")) {
      paths.push(await path.getText());
    }
    assert.ok(
      !paths.some((path) => path.includes("ContosoNotes")),
      "a healthy entry was offered",
    );
    assert.equal(await $("#reg-cleaner-clean").getText(), "Clean (8)");
    await screenshot("cleaner-results");
  });
});
