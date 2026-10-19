/**
 * Several programs at once: Ctrl+click picks them, one confirmation starts
 * them, and they are uninstalled one at a time, in list order, the next
 * only after the person is done with the last.
 */
import { strict as assert } from "node:assert";

import {
  clearSearch,
  programNames,
  ready,
  registryKeys,
  restorePoints,
  screenshot,
  waitForButton,
} from "./support.ts";

const UNINSTALL = String.raw`HKLM\software\microsoft\windows\currentversion\uninstall`;
const HELPER_KEY = String.raw`${UNINSTALL}\acmehelper`;
const WOODGROVE_KEY = String.raw`${UNINSTALL}\{b0b0b0b0-0000-0000-0000-00000000b0b0}`;

const row = (id: string) => $(`#un-program-rows tr[data-id*="${id}"]`);

/** A real Ctrl+click: Ctrl is held down while the mouse clicks the row. */
async function ctrlClick(id: string): Promise<void> {
  const target = await row(id);
  await browser.actions([
    browser.action("key").down("").pause(0).pause(0).pause(0).up(""),
    browser.action("pointer").pause(0).move({ origin: target }).down().up(),
  ]);
}

async function selected(id: string): Promise<boolean> {
  return (await (await row(id)).getAttribute("aria-selected")) === "true";
}

/** Past this program's leftovers (kept), to the step that ends it. */
async function finishProgram(name: string, place: string): Promise<void> {
  await browser.waitUntil(
    async () =>
      (await $("#un-wizard-skip").isClickable()) ||
      (await $("#un-wizard-next").isClickable()) ||
      (await $("#un-wizard-close").isClickable()),
    { timeout: 60_000, timeoutMsg: `${name} never reached its leftovers` },
  );
  assert.match(
    await $(".dialog h2").getText(),
    new RegExp(`${name}.*\\(${place}\\)$`),
  );
  if (await $("#un-wizard-skip").isClickable())
    await $("#un-wizard-skip").click();
}

describe("uninstalling several programs", () => {
  it("selects them with Ctrl+click", async () => {
    await ready();
    await clearSearch();
    await (await row("AcmeHelper")).click();
    await ctrlClick("B0B0B0B0");
    assert.ok(await selected("AcmeHelper"));
    assert.ok(await selected("B0B0B0B0"));
    assert.equal(await $("#un-uninstall").getText(), "Uninstall 2 programs");
    assert.match(await $("#un-details").getText(), /2 programs/);
    // Ctrl+click again takes one out, and once more puts it back.
    await ctrlClick("B0B0B0B0");
    assert.ok(!(await selected("B0B0B0B0")));
    assert.equal(await $("#un-uninstall").getText(), "Uninstall");
    await ctrlClick("B0B0B0B0");
    assert.ok(await selected("B0B0B0B0"));
    await screenshot("batch-selected");
  });

  it("confirms once, then uninstalls them one at a time", async () => {
    await $("#un-uninstall").click();
    await $("#un-wizard-start").waitForDisplayed({ timeout: 10_000 });
    assert.equal(await $("#un-wizard-start").getText(), "Uninstall 2 programs");
    const order = await $("#un-batch-programs").getText();
    assert.ok(
      order.indexOf("Acme Widget Helper") < order.indexOf("Woodgrove Runtime"),
      order,
    );
    await $("#un-mode-safe").click();
    // One restore point, named for the whole batch.
    if (!(await $("#un-wizard-restore").isSelected()))
      await $("#un-wizard-restore").click();
    await screenshot("batch-confirm");
    const earlierPoints = restorePoints().length;
    await $("#un-wizard-start").click();

    await finishProgram("Acme Widget Helper", "1 of 2");
    // Only the first program's uninstaller has run so far.
    let keys = registryKeys();
    assert.ok(!keys.includes(HELPER_KEY), "the first uninstaller ran");
    assert.ok(keys.includes(WOODGROVE_KEY), "the second waited its turn");
    await (await waitForButton("wizard-next")).click();

    await finishProgram("Woodgrove Runtime", "2 of 2");
    keys = registryKeys();
    assert.ok(!keys.includes(WOODGROVE_KEY), "the second uninstaller ran");
    assert.deepEqual(restorePoints().slice(earlierPoints), [
      "Before uninstalling Acme Widget Helper and 1 more program",
    ]);
    await (await waitForButton("wizard-close")).click();

    const summary = await $("#un-batch-summary");
    await summary.waitForDisplayed({ timeout: 10_000 });
    const text = await summary.getText();
    assert.match(text, /Acme Widget Helper/);
    assert.match(text, /Woodgrove Runtime/);
    await screenshot("batch-summary");
    await (await waitForButton("batch-close")).click();
    await $(".dialog").waitForExist({ reverse: true, timeout: 5_000 });
  });

  it("drops both from the list", async () => {
    await browser.waitUntil(
      async () => {
        const names = await programNames();
        return (
          !names.includes("Acme Widget Helper") &&
          !names.includes("Woodgrove Runtime")
        );
      },
      { timeout: 10_000, timeoutMsg: "the list did not refresh" },
    );
  });
});
