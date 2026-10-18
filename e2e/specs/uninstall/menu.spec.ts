/**
 * Right-click never opens the browser's menu: this is a desktop app. On
 * text it opens Windows' own edit menu, read here through Windows itself.
 */
import { strict as assert } from "node:assert";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { application } from "../../wdio.conf.ts";
import { appPids } from "./workspace.ts";
import { clearSearch, clickTab, ready } from "./support.ts";

const helper = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../native-menu.ps1",
);

/**
 * Right-click an element as a person would and say which menu the page let
 * through: "none" when it refused the browser's, "browser" when it did not,
 * undefined when no right-click reached the page at all.
 */
async function rightClick(selector: string): Promise<string | undefined> {
  await browser.execute(() => {
    delete document.documentElement.dataset["menu"];
    window.addEventListener(
      "contextmenu",
      (event) => {
        document.documentElement.dataset["menu"] = event.defaultPrevented
          ? "none"
          : "browser";
      },
      { once: true },
    );
  });
  await $(selector).click({ button: "right" });
  return browser.execute(() => document.documentElement.dataset["menu"]);
}

/**
 * The Windows menu the app has open, closed again with Esc: "Cut Ctrl+X"
 * per enabled item, "(Cut Ctrl+X)" per greyed one, "-" per separator, and
 * an empty list when no menu opens within `waitMs`.
 */
function nativeMenu(waitMs = 5_000): string[] {
  const output = execFileSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      helper,
      "-ProcessIds",
      appPids(application).join(","),
      "-WaitMs",
      String(waitMs),
    ],
    { encoding: "utf8" },
  );
  const items: unknown = JSON.parse(output);
  assert.ok(
    Array.isArray(items) && items.every((item) => typeof item === "string"),
    output,
  );
  return items;
}

describe("right-click", () => {
  it("opens no menu at all on a list, a button, plain text or a choice", async () => {
    await ready();
    await browser.execute(() => document.getSelection()?.removeAllRanges());
    for (const place of [
      "#un-program-rows tr",
      "#nav-unbackups",
      "#un-program-count",
    ]) {
      assert.equal(await rightClick(place), "none", place);
    }
    await clickTab("settings");
    assert.equal(await rightClick("#set-theme"), "none", "#set-theme");
    assert.deepEqual(nativeMenu(1_500), [], "no Windows menu either");
  });

  it("offers Windows' edit menu on a field, greyed where it does not apply", async () => {
    await clickTab("programs");
    await clearSearch();
    assert.equal(await rightClick("#un-search"), "none");
    assert.deepEqual(nativeMenu(), [
      "(Cut Ctrl+X)",
      "(Copy Ctrl+C)",
      "Paste Ctrl+V",
      "-",
      "(Select All Ctrl+A)",
    ]);

    await $("#un-search").setValue("Acme");
    await browser.keys(["Control", "a"]);
    assert.equal(await rightClick("#un-search"), "none");
    assert.deepEqual(nativeMenu(), [
      "Cut Ctrl+X",
      "Copy Ctrl+C",
      "Paste Ctrl+V",
      "-",
      "Select All Ctrl+A",
    ]);
    await clearSearch();
  });

  it("offers Copy alone for text selected outside a field", async () => {
    await browser.execute(() => {
      const count = document.getElementById("un-program-count");
      if (count) document.getSelection()?.selectAllChildren(count);
    });
    assert.equal(await rightClick("#un-program-count"), "none");
    assert.deepEqual(nativeMenu(), ["Copy Ctrl+C"]);
    await browser.execute(() => document.getSelection()?.removeAllRanges());
  });

  it("opens no browser menu in a dialog, which keeps the keyboard", async () => {
    await $("#un-program-rows tr").click();
    await browser.keys(["Enter"]);
    await $("#un-wizard-start").waitForDisplayed({ timeout: 10_000 });
    assert.equal(
      await rightClick(".dialog h2"),
      "none",
      "the uninstall dialog",
    );
    // Clicking the dialog's text must not hand focus to the page behind it.
    await browser.keys(["Shift", "Tab"]);
    assert.ok(
      await browser.execute(
        () => document.activeElement?.closest(".dialog") != null,
      ),
      "Shift+Tab after a click stays in the dialog",
    );
    await browser.keys(["Escape"]);
    await $(".dialog").waitForExist({ reverse: true, timeout: 5_000 });
  });
});
