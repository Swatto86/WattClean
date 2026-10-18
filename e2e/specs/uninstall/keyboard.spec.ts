/** Keyboard use: arrows move the selection, Enter opens the wizard, Esc closes it. */
import { strict as assert } from "node:assert";

import { ready } from "./support.ts";

describe("keyboard", () => {
  it("moves through the list and opens and cancels an uninstall", async () => {
    await ready();
    await $("#un-program-rows tr").click();
    await browser.keys(["ArrowDown", "ArrowDown"]);
    const selected = await $('#un-program-rows tr[aria-selected="true"]');
    assert.match(await selected.getText(), /Contoso Notes/);

    await browser.keys(["Enter"]);
    await $("#un-wizard-start").waitForDisplayed({ timeout: 10_000 });
    assert.match(await $(".dialog h2").getText(), /Uninstall Contoso Notes\?/);
    await browser.keys(["Escape"]);
    await $(".dialog").waitForExist({ reverse: true, timeout: 5_000 });

    const history = await browser.execute(
      () => document.activeElement?.tagName,
    );
    assert.equal(history, "TR", "focus returns to the list");
  });

  it("switches pages with the arrow keys in the navigation", async () => {
    await $("#nav-programs").click();
    await browser.keys(["ArrowDown"]);
    assert.ok(await $("#un-view-traced").isDisplayed());
    await browser.keys(["ArrowUp"]);
    assert.ok(await $("#un-view-programs").isDisplayed());
  });
});
