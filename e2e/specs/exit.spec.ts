/** A clean exit: with "keep running" off, closing the window ends the
 * process; so does a clean with "Exit after cleaning" on. */
import { strict as assert } from "node:assert";

import { application } from "../wdio.conf.ts";
import { appPids, readJson } from "../workspace.ts";
import { dialogButton, openPage, ready, tickOnly } from "./support.ts";

const gone = async (what: string): Promise<void> => {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline && appPids(application).length > 0) {
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  assert.deepEqual(
    appPids(application),
    [],
    `${what}: the app is still running`,
  );
};

describe("quitting", () => {
  it("ends the process when the window closes", async () => {
    await ready();
    await openPage("settings");
    await $("#set-tray").click();
    await browser.waitUntil(
      async () => {
        const saved = readJson("settings.json");
        return (
          typeof saved === "object" &&
          saved !== null &&
          Reflect.get(saved, "keepInTray") === false
        );
      },
      { timeout: 5_000, timeoutMsg: "keep running was not switched off" },
    );
    assert.ok(appPids(application).length > 0, "the app process was not found");
    // The title bar's X sends this same close request. Deferred so the
    // script returns before the window (and session) goes.
    await browser.execute(() => {
      const internals: unknown = Reflect.get(window, "__TAURI_INTERNALS__");
      const invoke: unknown =
        typeof internals === "object" && internals !== null
          ? Reflect.get(internals, "invoke")
          : undefined;
      if (typeof invoke !== "function") throw new Error("no Tauri bridge");
      setTimeout(() => invoke("plugin:window|close", { label: "main" }), 0);
    });
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline && appPids(application).length > 0) {
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    assert.deepEqual(appPids(application), [], "the app is still running");
    // A fresh session so the runner's teardown has something to close.
    await browser.reloadSession();
    await ready();
  });

  it("ends after a clean when Exit after cleaning is on", async () => {
    await openPage("settings");
    await $("#set-exit").click();
    await browser.waitUntil(
      async () => {
        const saved = readJson("disk/settings.json");
        return (
          typeof saved === "object" &&
          saved !== null &&
          Reflect.get(saved, "exitAfterClean") === true
        );
      },
      { timeout: 5_000, timeoutMsg: "the setting was not saved" },
    );
    await openPage("cleaner");
    await tickOnly(["system.clipboard"]);
    await $("#cleaner-clean").click();
    await dialogButton("Clean");
    await gone("after the clean");
    await browser.reloadSession();
    await ready();
  });
});
