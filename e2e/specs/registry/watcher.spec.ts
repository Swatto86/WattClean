/** A watched key logs the changes made beneath it. */
import { strict as assert } from "node:assert";

import { clickTab, nativeMenu, rightClick, screenshot } from "./support.ts";

const ADVANCED = String.raw`HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced`;

describe("the Registry Watcher", () => {
  it("refuses a key that does not exist", async () => {
    await clickTab("watcher");
    await $("#reg-watch-key").setValue(String.raw`HKCU\Software\Nothing Here`);
    await $("#reg-watch-add").click();
    await browser.waitUntil(
      async () => (await $("#reg-watch-error").getText()) !== "",
      { timeoutMsg: "no error for a missing key" },
    );
  });

  it("logs a change made while watching", async () => {
    await $("#reg-watch-key").setValue(ADVANCED);
    await $("#reg-watch-add").click();
    await $("#reg-watch-list .watch-item").waitForDisplayed({
      timeout: 10_000,
    });
    assert.match(
      await $("#reg-watch-list .watch-item").getText(),
      /HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced/,
    );
    await clickTab("tuner");
    await $("#reg-tweak-secondsInClock").click();
    await browser.waitUntil(async () =>
      $("#reg-tweak-secondsInClock").isSelected(),
    );
    await clickTab("watcher");
    await browser.waitUntil(
      async () =>
        (await $("#reg-watch-events").getText()).includes(
          "ShowSecondsInSystemClock",
        ),
      { timeout: 15_000, timeoutMsg: "the change was not logged" },
    );
    const row = await $("#reg-watch-events tr").getText();
    assert.match(row, /Value added/);
    assert.match(row, /0x00000001 \(1\)/);
    // The app made the change itself, and tracing named it.
    assert.match(row, /wattclean\.exe/);
    assert.equal(await $("#reg-watch-tracing").isDisplayed(), false);
    await screenshot("watcher");
  });

  it("offers the log's actions in a native menu", async () => {
    assert.equal(await rightClick("#reg-watch-events tr"), "none");
    assert.deepEqual(nativeMenu(), [
      "Open in Registry Editor",
      "Copy key path",
      "Copy new data",
      "-",
      "Export log…",
      "Clear log",
    ]);
  });
});
