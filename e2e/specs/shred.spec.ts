/** Shredding chosen files. */
import { strict as assert } from "node:assert";
import fs from "node:fs";

import { machinePath, setPicks } from "../workspace.ts";
import { dialogButton, dialogText, openPage, screenshot } from "./support.ts";

const SECRET = String.raw`Users\Tester\Documents\Secret`;

describe("shredding", () => {
  it("overwrites and deletes the files added", async () => {
    const plan = machinePath(`${SECRET}\\plan.txt`);
    assert.ok(fs.existsSync(plan));
    setPicks({ files: [plan] });
    await openPage("shred");
    await $("#shred-add-files").click();
    await browser.waitUntil(
      async () => (await $("#shred-list").getText()).includes("plan.txt"),
      { timeout: 10_000, timeoutMsg: "the file was not added to the list" },
    );
    await screenshot("shred-list");
    await $("#shred-start").click();
    assert.match(await dialogText(), /Shred these\?/);
    await dialogButton("Shred");
    await browser.waitUntil(
      async () =>
        /1 file and 0 folders shredded/.test(
          await $("#shred-outcome").getText(),
        ),
      { timeout: 30_000, timeoutMsg: "the shred never reported" },
    );
    assert.ok(!fs.existsSync(plan), "the file is gone");
    assert.deepEqual(
      fs.readdirSync(machinePath(SECRET)),
      [],
      "no renamed copy left",
    );
  });
});
