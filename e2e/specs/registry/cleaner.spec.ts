/** Cleaning removes exactly the ticked entries, after a restore point. */
import { strict as assert } from "node:assert";

import {
  dialogButton,
  dialogText,
  dialogTitled,
  hasKey,
  machine,
  registryValue,
  screenshot,
  summaryOf,
} from "./support.ts";

const RUN = String.raw`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`;

describe("cleaning", () => {
  it("scans and goes straight to the clean dialog (Quick Clean)", async () => {
    await $("#reg-cleaner-again").click();
    await $("#reg-cleaner-quick").click();
    assert.match(await dialogTitled("Remove entries"), /Remove 8 entries/);
    assert.ok(await $("#reg-clean-full-backup").isExisting());
    await dialogButton("Cancel");
    assert.equal(await summaryOf("cleaner"), "8 entries found, 8 ticked.");
  });

  it("asks first, offering a restore point", async () => {
    await $("#reg-cleaner-clean").click();
    await $("#reg-clean-restore-point").waitForExist({ timeout: 10_000 });
    assert.ok(await $("#reg-clean-restore-point").isSelected());
    await screenshot("clean-confirm");
    await dialogButton("Remove");
    const text = await dialogText();
    assert.match(text, /8 entries were removed\./);
    assert.match(text, /Backup Center/);
    await dialogButton("OK");
  });

  it("removed the broken entries and left the healthy ones", async () => {
    assert.equal(registryValue(RUN, "FabrikamUpdater"), undefined);
    assert.ok(
      registryValue(RUN, "ContosoNotes"),
      "a healthy entry was removed",
    );
    assert.ok(
      !hasKey(String.raw`HKCU\Software\Classes\Directory\shell\FabrikamScan`),
    );
    assert.ok(
      registryValue(RUN, "PortableTool"),
      "an entry on an unplugged drive was removed",
    );
    assert.deepEqual(machine().restore_points, [
      "WattClean: before cleaning the registry",
    ]);
    assert.equal(
      await summaryOf("cleaner"),
      "The registry is clean: nothing was found.",
    );
  });

  it("a second Safe scan finds nothing more", async () => {
    await $("#reg-cleaner-again").click();
    await $("#reg-cleaner-scan").click();
    await $("#reg-cleaner-results").waitForDisplayed({ timeout: 60_000 });
    await browser.waitUntil(
      async () =>
        (await summaryOf("cleaner")) ===
        "The registry is clean: nothing was found.",
      { timeout: 30_000 },
    );
  });
});
