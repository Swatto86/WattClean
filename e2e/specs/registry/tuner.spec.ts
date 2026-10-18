/** Tweaks switch on and back off to exactly what was there. */
import { strict as assert } from "node:assert";

import {
  clickTab,
  dialogButton,
  dialogTitled,
  machine,
  registryValue,
  screenshot,
  untilMachine,
} from "./support.ts";

const ADVANCED = String.raw`HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced`;

/** The value's data as the fake machine stores it: little-endian hex. */
const hideFileExt = () => registryValue(ADVANCED, "HideFileExt")?.bytes;

describe("the Registry Tuner", () => {
  it("shows each tweak with its current state", async () => {
    await clickTab("tuner");
    await $("#reg-tweak-showExtensions").waitForExist({ timeout: 10_000 });
    assert.equal(await $("#reg-tweak-showExtensions").isSelected(), false);
    assert.equal(hideFileExt(), "01000000");
    await screenshot("tuner");
  });

  it("switches a tweak on, marked as changed by the app", async () => {
    await $("#reg-tweak-showExtensions").click();
    await untilMachine(() => hideFileExt() === "00000000", "HideFileExt = 0");
    const row = await $('.tweak[data-id="showExtensions"]');
    await browser.waitUntil(async () =>
      (await row.getText()).includes("Changed by WattClean"),
    );
    assert.match(
      await $("#reg-tuner-pending").getText(),
      /restarting File Explorer/,
    );
  });

  it("restarts File Explorer when asked", async () => {
    await $("#reg-tuner-explorer").click();
    await dialogButton("Restart");
    await untilMachine(
      () => machine().explorer_restarts === 1,
      "File Explorer restarted",
    );
  });

  it("signs out or restarts Windows only when the person agrees", async () => {
    await $("#reg-tuner-sign-out").click();
    assert.match(await dialogTitled("Sign out"), /save your work first/);
    await dialogButton("Cancel");
    await $("#reg-tuner-restart").click();
    assert.match(await dialogTitled("Restart PC"), /save your work first/);
    await dialogButton("Restart PC");
    await untilMachine(
      () => machine().pc_restarts === 1,
      "a restart asked for",
    );
    assert.equal(
      machine().sign_outs ?? 0,
      0,
      "the cancelled sign-out happened",
    );
  });

  it("switches back to exactly what was there", async () => {
    await $("#reg-tweak-showExtensions").click();
    await untilMachine(() => hideFileExt() === "01000000", "HideFileExt = 1");
    assert.equal(await $("#reg-tweak-showExtensions").isSelected(), false);
  });

  it("filters by words", async () => {
    await $("#reg-tuner-filter").setValue("taskbar clock");
    await browser.waitUntil(async () => (await $$(".tweak").length) === 1, {
      timeoutMsg: "the filter did not narrow the list",
    });
    await $("#reg-tuner-filter").clearValue();
  });
});
