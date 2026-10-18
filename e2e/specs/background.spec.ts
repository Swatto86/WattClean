/**
 * Running in the notification area (on by default): closing the window
 * hides it and the process stays; a second launch shows the running
 * window; Quit ends it. A sign-in start stays hidden. Starting with Windows
 * is refused for a development build.
 */
import { strict as assert } from "node:assert";
import { execFileSync, spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { application } from "../wdio.conf.ts";
import { appPids, stopApp } from "../workspace.ts";
import { openPage, ready, restartApp } from "./support.ts";

const probe = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../window-visible.ps1",
);

/** Whether the app's own window ("WattClean") is visible, asked of Windows. */
export function windowShown(): boolean {
  const output = execFileSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      probe,
      "-ProcessIds",
      appPids(application).join(","),
      "-Title",
      "WattClean",
    ],
    { encoding: "utf8" },
  );
  return output.trim() === "true";
}

/** Sends the close request the title bar's X sends. */
export async function closeWindow(): Promise<void> {
  // Deferred so the script returns before the window (and session) goes.
  await browser.execute(() => {
    const internals: unknown = Reflect.get(window, "__TAURI_INTERNALS__");
    const invoke: unknown =
      typeof internals === "object" && internals !== null
        ? Reflect.get(internals, "invoke")
        : undefined;
    if (typeof invoke !== "function") throw new Error("no Tauri bridge");
    setTimeout(() => invoke("plugin:window|close", { label: "main" }), 0);
  });
}

const launch = (args: string[]) =>
  spawn(application, args, {
    detached: true,
    stdio: "ignore",
    env: process.env,
  }).unref();

const settle = () => new Promise((resolve) => setTimeout(resolve, 3_000));

describe("running in the background", () => {
  it("refuses to start with Windows from a development build", async () => {
    await ready();
    await openPage("settings");
    await browser.waitUntil(async () =>
      /development build/.test(await $("#reg-autostart-problem").getText()),
    );
    assert.equal(await $("#reg-set-autostart").isEnabled(), false);
    assert.ok(await $("#set-tray").isSelected(), "keep running starts on");
  });

  it("keeps running, hidden, when the window closes", async () => {
    assert.ok(windowShown(), "the window should be showing first");
    await closeWindow();
    await browser.waitUntil(async () => !windowShown(), {
      timeout: 10_000,
      timeoutMsg: "the window did not hide",
    });
    await new Promise((resolve) => setTimeout(resolve, 1_500));
    assert.ok(appPids(application).length > 0, "the app quit instead");
  });

  it("shows the running window again when opened a second time", async () => {
    launch([]);
    await browser.waitUntil(async () => windowShown(), {
      timeout: 15_000,
      timeoutMsg: "opening it again did not show the running window",
    });
    await settle();
    assert.equal(appPids(application).length, 1, "opening it made a copy");
  });

  it("ends when Quit is chosen", async () => {
    await browser.execute(() => {
      setTimeout(() => document.getElementById("quit-app")?.click(), 0);
    });
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline && appPids(application).length > 0) {
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    assert.deepEqual(appPids(application), [], "the app is still running");
    await browser.reloadSession();
    await ready();
  });

  it("starts hidden at sign-in, and a second launch reuses it", async () => {
    // As Windows starts it from the Run entry: the suite's own binary,
    // with the suite's isolated data folder.
    await stopApp(application);
    launch(["--background"]);
    await browser.waitUntil(async () => appPids(application).length === 1, {
      timeout: 15_000,
      timeoutMsg: "the background start did not run",
    });
    await settle();
    assert.equal(windowShown(), false, "a sign-in start showed its window");
    launch(["--background"]);
    await settle();
    assert.equal(appPids(application).length, 1, "a second copy kept running");
    assert.equal(windowShown(), false, "a second sign-in start showed it");
    launch([]);
    await browser.waitUntil(async () => windowShown(), {
      timeout: 15_000,
      timeoutMsg: "opening it again did not show the running window",
    });
    await settle();
    assert.equal(appPids(application).length, 1, "opening it made a copy");
    await restartApp();
  });
});
