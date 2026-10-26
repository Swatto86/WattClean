import { strict as assert } from "node:assert";
import { dialogButton, ready, screenshot } from "./support.ts";

describe("Windows Store apps", () => {
  it("lists, searches and cancels without removing a package", async () => {
    await ready();
    await $("#un-store-apps").click();
    await $("#un-store-search").waitForDisplayed();
    assert.equal(await $("#un-store-count").getText(), "2 Store apps");
    await $("#un-store-search").setValue("Notes");
    assert.equal(await $("#un-store-count").getText(), "1 Store app");
    await $("#un-store-rows button").click();
    assert.match(
      await $("#un-dialog-root").getText(),
      /cannot back up or restore Store app data/,
    );
    await dialogButton("Cancel");
    assert.match(await $("#un-store-rows").getText(), /Contoso Store Notes/);
  });

  it("removes the selected package through IPC and refreshes", async () => {
    await $("#un-store-rows button").click();
    await $("#un-store-confirm").click();
    await $("#un-store-search").waitForDisplayed();
    assert.match(await $("#un-store-notice").getText(), /Notes was removed/);
    assert.equal(await $("#un-store-count").getText(), "0 Store apps");
    await $("#un-store-search").click();
    await browser.keys(["Control", "a"]);
    await browser.keys(["Backspace"]);
    assert.equal(await $("#un-store-count").getText(), "1 Store app");
    assert.doesNotMatch(await $("#un-store-rows").getText(), /Store Notes/);
  });

  it("shows Windows failures and keeps the refused package", async () => {
    await $("#un-store-rows button").click();
    await $("#un-store-confirm").click();
    await $("#un-store-search").waitForDisplayed();
    assert.match(await $("#un-store-notice").getText(), /Windows refused/);
    assert.match(await $("#un-store-rows").getText(), /Contoso Store Blocked/);
    await screenshot("store-apps");
    await dialogButton("Close");
  });
});
