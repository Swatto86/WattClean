/**
 * Right-click never opens the browser's menu. Lists open Windows' own popup
 * menu, read and driven here through Windows itself; text fields get
 * Windows' edit menu.
 */
import { strict as assert } from "node:assert";

import {
  chooseFromMenu,
  clickTab,
  machine,
  nativeMenu,
  rightClick,
  summaryOf,
} from "./support.ts";

describe("right-click", () => {
  it("opens the findings menu natively", async () => {
    assert.equal(await rightClick("#reg-cleaner-tree .finding"), "none");
    assert.deepEqual(nativeMenu(), [
      "Unselect this",
      "-",
      "Select all",
      "Select none",
      "-",
      "Expand all",
      "Collapse all",
      "-",
      "Open in Registry Editor",
      "Copy key path",
      "Add key to exclude list",
      'Search Google for "GhostApp"',
      "-",
      "Export list…",
    ]);
  });

  it("acts on the item chosen from the native menu", async () => {
    await chooseFromMenu("#reg-cleaner-tree .finding", "Unselect this");
    await browser.waitUntil(
      async () => (await summaryOf("cleaner")) === "8 entries found, 7 ticked.",
      { timeout: 5_000, timeoutMsg: "unticking from the menu did nothing" },
    );
    await chooseFromMenu("#reg-cleaner-tree .finding", "Select all");
    await browser.waitUntil(
      async () => (await summaryOf("cleaner")) === "8 entries found, 8 ticked.",
    );
    const key = await $("#reg-cleaner-tree .finding .path").getText();
    await chooseFromMenu(
      "#reg-cleaner-tree .finding",
      "Open in Registry Editor",
    );
    await browser.waitUntil(async () => machine().opened.includes(key), {
      timeout: 5_000,
      timeoutMsg: `Registry Editor was not asked to open ${key}`,
    });
    await chooseFromMenu("#reg-cleaner-tree .finding", "Search Google");
    await browser.waitUntil(
      async () =>
        (machine().opened_urls ?? []).includes(
          "https://www.google.com/search?q=GhostApp",
        ),
      { timeout: 5_000, timeoutMsg: "no web search was opened" },
    );
  });

  it("collapses a group from the menu and keeps the keyboard", async () => {
    await chooseFromMenu("#reg-cleaner-tree .group-row", "Collapse all");
    await browser.waitUntil(
      async () => (await $$("#reg-cleaner-tree .finding").length) === 0,
    );
    await $("#reg-cleaner-tree .group-row").click();
    await browser.keys(["ArrowRight"]);
    await browser.waitUntil(
      async () => (await $$("#reg-cleaner-tree .finding").length) > 0,
    );
    await chooseFromMenu("#reg-cleaner-tree .group-row", "Expand all");
    await browser.waitUntil(
      async () => (await $$("#reg-cleaner-tree .finding").length) === 8,
    );
  });

  it("opens no menu at all on plain text or a button", async () => {
    await browser.execute(() => document.getSelection()?.removeAllRanges());
    for (const place of ["#reg-cleaner-summary", "#nav-regbackups", "h1"]) {
      assert.equal(await rightClick(place), "none", place);
    }
    assert.deepEqual(nativeMenu(1_500), [], "no Windows menu either");
  });

  it("offers Windows' edit menu on a text field", async () => {
    await clickTab("search");
    await $("#reg-search-text").waitForDisplayed();
    assert.equal(await rightClick("#reg-search-text"), "none");
    assert.deepEqual(nativeMenu(), [
      "(Cut Ctrl+X)",
      "(Copy Ctrl+C)",
      "Paste Ctrl+V",
      "-",
      "(Select All Ctrl+A)",
    ]);
    await clickTab("cleaner");
  });
});
