/**
 * A key Windows protects is refused, then removed by taking ownership, and
 * restoring it brings back its protection.
 */
import { strict as assert } from "node:assert";

import {
  chooseFromMenu,
  clickTab,
  dialogButton,
  dialogText,
  hasKey,
  keySecurity,
} from "./support.ts";

const PROTECTED = String.raw`HKLM\SOFTWARE\Classes\CLSID\{F00DFACE-0000-4000-8000-000000000004}`;

/** Tick only the protected entry in the cleaner's results. */
async function tickOnlyProtected(): Promise<void> {
  await chooseFromMenu("#reg-cleaner-tree .finding", "Select none");
  for (const row of await $$("#reg-cleaner-tree .finding")) {
    if ((await row.$(".path").getText()) === PROTECTED) {
      await row.$("input").click();
      return;
    }
  }
  throw new Error("the protected entry was not listed");
}

describe("protected keys", () => {
  it("are found by a Moderate scan", async () => {
    await clickTab("cleaner");
    await $("#reg-cleaner-again").click();
    await $('input[name="mode"][value="moderate"]').click();
    await $("#reg-cleaner-scan").click();
    await $("#reg-cleaner-results").waitForDisplayed({ timeout: 60_000 });
    await tickOnlyProtected();
    await browser.waitUntil(
      async () => (await $("#reg-cleaner-clean").getText()) === "Clean (1)",
    );
  });

  it("are refused without taking ownership", async () => {
    await $("#reg-cleaner-clean").click();
    await $("#reg-clean-force").waitForExist({ timeout: 10_000 });
    assert.equal(
      await $("#reg-clean-force").isSelected(),
      false,
      "off by default",
    );
    await dialogButton("Remove");
    const text = await dialogText();
    assert.match(text, /0 entries were removed\./);
    assert.match(text, /Not removed: .*000000000004.*Take ownership/s);
    await dialogButton("OK");
    assert.ok(hasKey(PROTECTED));
  });

  it("are removed by taking ownership, keeping their permissions", async () => {
    await $("#reg-cleaner-clean").click();
    await $("#reg-clean-force").waitForExist({ timeout: 10_000 });
    await $("#reg-clean-force").click();
    await dialogButton("Remove");
    assert.match(
      await dialogText(),
      /1 entry was removed \(1 after taking ownership\)\./,
    );
    await dialogButton("OK");
    assert.ok(!hasKey(PROTECTED), "the protected key is still there");
  });

  it("come back protected when restored", async () => {
    await clickTab("backups");
    const newest = await $("#reg-backup-list article");
    await browser.waitUntil(async () =>
      (await newest.getText()).includes("Registry clean, 1 entry"),
    );
    await newest.$("button=Restore").click();
    await dialogButton("Restore");
    assert.match(await dialogText(), /1 entry restored\./);
    await dialogButton("OK");
    assert.ok(hasKey(PROTECTED), "it was not restored");
    assert.match(keySecurity(PROTECTED) ?? "", /^O:S-1-5-80-/);
    assert.match(
      keySecurity(`${PROTECTED}\\InprocServer32`) ?? "",
      /^O:S-1-5-80-/,
    );
  });
});
