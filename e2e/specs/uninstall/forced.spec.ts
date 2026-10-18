/** Forced uninstall by name and folder, for a program with no uninstaller. */
import { strict as assert } from "node:assert";
import fs from "node:fs";

import { machinePath } from "./workspace.ts";
import {
  clickTab,
  dialogButton,
  leftoverPaths,
  ready,
  waitForButton,
} from "./support.ts";

describe("forced uninstall", () => {
  it("refuses a folder that does not exist", async () => {
    await ready();
    await clickTab("forced");
    await $("#un-forced-name").setValue("Contoso Notes");
    await $("#un-forced-folder").setValue("Z:\\no\\such\\folder");
    await $("#un-forced-scan").click();
    await browser.waitUntil(
      async () => (await $("#un-forced-error").getText()) !== "",
    );
    assert.match(await $("#un-forced-error").getText(), /existing folder/);
  });

  it("scans by name and folder and removes what it finds", async () => {
    const folder = machinePath(
      "Users\\tester\\AppData\\Local\\Programs\\Contoso Notes",
    );
    await $("#un-forced-folder").setValue(folder);
    await $("#un-forced-scan").click();
    const paths = await leftoverPaths();
    for (const ending of [
      "AppData\\Local\\Programs\\Contoso Notes",
      "AppData\\Roaming\\Contoso Notes",
      "HKCU\\Software\\ContosoNotes",
    ]) {
      assert.ok(
        paths.some((p) => p.endsWith(ending)),
        `missing ${ending}`,
      );
    }
    await (await waitForButton("wizard-remove")).click();
    await dialogButton("Remove");
    await $("#un-removal-summary").waitForDisplayed({ timeout: 60_000 });
    await (await waitForButton("wizard-close")).click();
    assert.ok(!fs.existsSync(folder));
  });
});
