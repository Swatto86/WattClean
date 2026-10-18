/** The primary workflow: tick, preview, clean, and the history it leaves. */
import { strict as assert } from "node:assert";
import fs from "node:fs";

import { machinePath, readJson } from "../workspace.ts";
import {
  dialogButton,
  dialogText,
  machineState,
  openPage,
  screenshot,
  tickOnly,
} from "./support.ts";

const TEMP = String.raw`Users\Tester\AppData\Local\Temp`;
const CHROME_CACHE = String.raw`Users\Tester\AppData\Local\Google\Chrome\User Data\Default\Cache\Cache_Data\data_1`;
const VLC_INI = String.raw`Users\Tester\AppData\Roaming\vlc\vlc-qt-interface.ini`;
const RUN_MRU = String.raw`HKCU\software\microsoft\windows\currentversion\explorer\runmru`;
const PROJECT = String.raw`Users\Tester\Projects\demo`;
const NOT_A_BUILD = String.raw`Users\Tester\Projects\notes\target\plan.txt`;

describe("cleaning", () => {
  it("asks before ticking an option with a warning", async () => {
    await tickOnly([]);
    await $('.cleaner-row[data-cleaner="chrome"] .expander').click();
    await $("#opt-chrome-passwords").click();
    assert.match(await dialogText(), /Saved passwords/);
    await dialogButton("Cancel");
    assert.equal(await $("#opt-chrome-passwords").isSelected(), false);
  });

  it("previews without deleting anything", async () => {
    await tickOnly([
      "system.temp",
      "chrome.cache",
      "explorer.run",
      "vlc.recent",
    ]);
    assert.equal(await $("#status-selection").getText(), "4 options ticked");
    await $("#cleaner-preview").click();
    await $("#report-tiles").waitForDisplayed({ timeout: 60_000 });
    assert.match(
      await $("#report-summary").getText(),
      /Nothing has been deleted yet/,
    );
    const keys: string[] = [];
    for (const head of await $$("#option-results .result-head")) {
      keys.push(String(await head.getAttribute("data-key")));
    }
    assert.deepEqual(keys.sort(), [
      "chrome.cache",
      "explorer.run",
      "system.temp",
      "vlc.recent",
    ]);
    assert.ok(fs.existsSync(machinePath(`${TEMP}\\setup-1234.tmp`)));
    await $('#option-results .result-head[data-key="system.temp"]').click();
    const listed = await $(
      '#option-results .result-head[data-key="system.temp"] + .items',
    ).getText();
    assert.match(listed, /setup-1234\.tmp/);
    assert.doesNotMatch(listed, /in-use-today\.tmp/, "fresh files stay");
    await screenshot("preview");
  });

  it("cleans after confirming, and records it", async () => {
    await $("#cleaner-clean").click();
    assert.match(await dialogText(), /Clean now\?/);
    await dialogButton("Clean");
    await browser.waitUntil(
      async () => /^Cleaned/.test(await $("#report-summary").getText()),
      { timeout: 60_000, timeoutMsg: "the clean never finished" },
    );
    assert.ok(!fs.existsSync(machinePath(`${TEMP}\\setup-1234.tmp`)));
    assert.ok(fs.existsSync(machinePath(`${TEMP}\\in-use-today.tmp`)));
    assert.ok(!fs.existsSync(machinePath(CHROME_CACHE)));
    assert.ok(
      !(RUN_MRU in machineState().registry),
      "the Run history key was deleted",
    );
    const ini = fs.readFileSync(machinePath(VLC_INI), "utf8");
    assert.ok(!ini.includes("RecentsMRU") && ini.includes("qt-mode=1"), ini);
    const history = readJson("disk/history.json");
    assert.ok(
      Array.isArray(history) && history.length === 1,
      JSON.stringify(history),
    );
    await screenshot("cleaned");
    await openPage("history");
    await $("#history-table").waitForDisplayed({ timeout: 10_000 });
    assert.match(await $("#history-table").getText(), /Clean/);
    await screenshot("history");
    await openPage("cleaner");
  });

  it("ticks every option with All, after one look at the warnings", async () => {
    await tickOnly([]);
    await $("#select-all").click();
    assert.match(await dialogText(), /Tick every option\?/);
    // The list scrolls, so read all of it rather than what is in view.
    const warnings = String(
      await $("dialog.dialog[open] .dialog-list").getProperty("textContent"),
    );
    assert.match(warnings, /Google Chrome › Saved passwords/);
    assert.match(warnings, /Build output › Rust build folders/);
    await dialogButton("Cancel");
    assert.equal(await $("#status-selection").getText(), "0 options ticked");

    await $("#select-all").click();
    await dialogButton("Tick all");
    const [shown, ticked] = await browser.execute(() => {
      const boxes = Array.from(
        document.querySelectorAll<HTMLInputElement>(
          "#cleaner-tree input[data-key]",
        ),
      );
      return [boxes.length, boxes.filter((b) => b.checked).length];
    });
    assert.ok(shown > 20, `only ${shown} options shown`);
    assert.equal(ticked, shown);
    assert.equal(
      await $("#status-selection").getText(),
      `${shown} options ticked`,
    );
    await screenshot("select-all");
  });

  it("cleans a Rust project's build folder and nothing else", async () => {
    await tickOnly([]);
    await $('.cleaner-row[data-cleaner="build_output"] .expander').click();
    await $("#opt-build_output-rust").click();
    assert.match(await dialogText(), /compiled from scratch/);
    await dialogButton("Tick it");
    assert.equal(await $("#status-selection").getText(), "1 option ticked");
    await $("#cleaner-clean").click();
    await dialogButton("Clean");
    await browser.waitUntil(
      async () => /^Cleaned/.test(await $("#report-summary").getText()),
      { timeout: 60_000, timeoutMsg: "the clean never finished" },
    );
    assert.ok(!fs.existsSync(machinePath(`${PROJECT}\\target`)));
    assert.ok(fs.existsSync(machinePath(`${PROJECT}\\Cargo.toml`)));
    assert.ok(fs.existsSync(machinePath(`${PROJECT}\\src\\main.rs`)));
    assert.ok(fs.existsSync(machinePath(NOT_A_BUILD)));
  });
});
