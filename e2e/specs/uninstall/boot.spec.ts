/** Boot to the main window: the test machine's programs are listed. */
import { strict as assert } from "node:assert";

import { clearSearch, programNames, ready, screenshot } from "./support.ts";

describe("WattClean boots", () => {
  it("lists the installed programs, hiding updates and system components", async () => {
    await ready();
    const names = await programNames();
    assert.deepEqual(names, [
      "Acme Widget 2.1 (x64)",
      "Acme Widget Helper",
      "Contoso Notes",
      "Fabrikam Viewer",
      "Northwind Broken Tool",
      "Woodgrove Runtime",
    ]);
    assert.equal(await $("#un-program-count").getText(), "6 programs");
    assert.ok(await $("#test-machine").isDisplayed(), "test build is labelled");
    assert.equal(await $("#status-rights").getText(), "Administrator");
    assert.ok((await $("#un-uninstall").getAttribute("disabled")) !== null);
    await screenshot("programs");
  });

  it("filters by name or publisher and shows a program's details", async () => {
    await $("#un-search").setValue("fabrikam");
    await browser.waitUntil(async () => (await programNames()).length === 1);
    await $("#un-program-rows tr").click();
    const details = await $("#un-details").getText();
    assert.match(details, /Fabrikam, Inc\./);
    assert.match(details, /MsiExec\.exe \/I\{0A1B2C3D/);
    assert.equal(await $("#un-uninstall").getAttribute("disabled"), null);
    await clearSearch();
    await browser.waitUntil(async () => (await programNames()).length === 6);
  });

  it("sorts by a column and back", async () => {
    await $('th[data-sort="name"] button').click();
    assert.equal((await programNames())[0], "Woodgrove Runtime");
    assert.equal(
      await $('th[data-sort="name"]').getAttribute("aria-sort"),
      "descending",
    );
    await $('th[data-sort="name"] button').click();
    assert.equal((await programNames())[0], "Acme Widget 2.1 (x64)");
  });
});
