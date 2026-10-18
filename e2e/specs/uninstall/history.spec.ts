/** The uninstall is on record with its scan and what it removed. */
import { strict as assert } from "node:assert";

import { readJson } from "./workspace.ts";
import { clickTab, ready } from "./support.ts";

describe("history", () => {
  it("records the Acme Widget uninstall", async () => {
    await ready();
    await clickTab("history");
    const entry = await $("article[data-history]");
    await entry.waitForDisplayed({ timeout: 10_000 });
    const text = await entry.getText();
    assert.match(text, /Acme Widget 2\.1 \(x64\) 2\.1\.0/);
    assert.match(text, /Uninstaller ran/);
    assert.match(text, /moderate scan · Removed \d+ registry keys/);

    const saved = readJson("history.json");
    assert.ok(
      typeof saved === "object" && saved !== null && "entries" in saved,
    );
    assert.ok(Array.isArray(saved.entries) && saved.entries.length === 1);
  });
});
