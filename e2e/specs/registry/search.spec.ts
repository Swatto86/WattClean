/** Searching finds matches, and removes only the one ticked. */
import { strict as assert } from "node:assert";

import {
  clickTab,
  dialogButton,
  dialogText,
  hasKey,
  summaryOf,
} from "./support.ts";

const FABDOC = String.raw`HKCU\Software\Classes\.fabdoc`;

describe("searching", () => {
  it("refuses a search too short to be useful", async () => {
    await clickTab("search");
    await $("#reg-search-text").setValue("ab");
    await $("#reg-search-go").click();
    // The browser's own minlength check or the app's: either way, no results.
    assert.ok(!(await $("#reg-search-results").isDisplayed()));
  });

  it("finds matches unticked and removes only the ticked one", async () => {
    await $("#reg-search-text").setValue("Fabrikam.Document");
    await $("#reg-search-go").click();
    await $("#reg-search-results").waitForDisplayed({ timeout: 60_000 });
    assert.equal(
      await summaryOf("search"),
      "1 match, 0 ticked. Tick what to remove.",
    );
    assert.equal(await $("#reg-search-tree .finding .path").getText(), FABDOC);
    await $("#reg-search-tree .finding input").click();
    await $("#reg-search-delete").click();
    await dialogButton("Remove");
    assert.match(await dialogText(), /1 entry was removed\./);
    await dialogButton("OK");
    assert.ok(hasKey(FABDOC), "the key was removed, not just its value");
  });
});
