/** The disk cleaner's settings (the keep list, the ticked options) and the
 * shell's (theme, text size) survive a restart. */
import { strict as assert } from "node:assert";

import { machinePath, readJson, setPicks } from "../workspace.ts";
import { openPage, restartApp, tickOnly } from "./support.ts";

/** A field of a saved settings file: the shell's or the disk cleaner's. */
const field = (file: string, name: string): unknown => {
  const value = readJson(file);
  return typeof value === "object" && value !== null
    ? Reflect.get(value, name)
    : undefined;
};
const saved = (name: string): unknown => field("disk/settings.json", name);
const shellSaved = (name: string): unknown => field("settings.json", name);

describe("persistence across a restart", () => {
  it("saves each change at once", async () => {
    await tickOnly(["system.clipboard"]);
    await browser.waitUntil(
      async () =>
        JSON.stringify(saved("selection")) ===
        JSON.stringify(["system.clipboard"]),
      { timeout: 5_000, timeoutMsg: "the ticks were not saved" },
    );
    await openPage("settings");
    await $("#set-theme").selectByAttribute("value", "light");
    await browser.waitUntil(async () => shellSaved("theme") === "light", {
      timeout: 5_000,
      timeoutMsg: "the theme was not saved",
    });
    await $("#set-text-size").selectByAttribute("value", "large");
    await browser.waitUntil(async () => shellSaved("textSize") === "large", {
      timeout: 5_000,
      timeoutMsg: "the text size was not saved",
    });
    setPicks({ folder: machinePath(String.raw`Users\Tester\Documents`) });
    await $("#keep-list-add-folder").click();
    await browser.waitUntil(
      async () => {
        const keep = saved("keep");
        return Array.isArray(keep) && keep.length === 1;
      },
      { timeout: 5_000, timeoutMsg: "the keep list was not saved" },
    );
  });

  it("comes back the same after a restart", async () => {
    await restartApp();
    assert.equal(await $("html").getAttribute("data-theme"), "light");
    assert.equal(await $("#status-selection").getText(), "1 option ticked");
    await openPage("settings");
    assert.match(await $("#keep-list").getText(), /Documents/);
    assert.equal(await $("#set-theme").getValue(), "light");
    assert.equal(await $("html").getAttribute("data-text-size"), "large");
    await $("#set-text-size").selectByAttribute("value", "normal");
    await browser.waitUntil(async () => shellSaved("textSize") === "normal");
    await openPage("cleaner");
  });
});
