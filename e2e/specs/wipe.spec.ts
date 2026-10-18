/** Wiping free space (the test machine stops after 3 MB). */
import { strict as assert } from "node:assert";
import fs from "node:fs";

import { machinePath } from "../workspace.ts";
import { dialogButton, openPage, screenshot } from "./support.ts";

describe("wiping free space", () => {
  it("fills the drive's free space and gives it back", async () => {
    await openPage("wipe");
    const button = await $("#drives button[data-root]");
    await button.waitForDisplayed({ timeout: 10_000 });
    assert.equal(await button.getAttribute("data-root"), "C:\\");
    await screenshot("wipe");
    await button.click();
    await dialogButton("Wipe");
    await $("#wipe-result").waitForDisplayed({ timeout: 60_000 });
    assert.match(await $("#wipe-result").getText(), /Wiped: 3\.00 MB/);
    const leftovers = fs
      .readdirSync(machinePath(""))
      .filter((name) => name.startsWith("WattClean-wipe-"));
    assert.deepEqual(leftovers, [], "the filler was deleted");
  });
});
