/** Boot to the main window on the test machine. */
import { strict as assert } from "node:assert";

import { ready, screenshot } from "./support.ts";

describe("WattClean boots", () => {
  it("opens on the Cleaner for the test machine", async () => {
    await ready();
    assert.ok(await $("#page-cleaner").isDisplayed());
    assert.ok(await $("#test-machine").isDisplayed(), "test build is labelled");
    assert.equal(await $("#status-rights").getText(), "Administrator");
    const groups: string[] = [];
    for (const title of await $$("#cleaner-tree .group-title")) {
      groups.push(await title.getText());
    }
    assert.ok(
      groups.some((g) => /windows/i.test(g)),
      groups.join(),
    );
    assert.ok(
      groups.some((g) => /browsers/i.test(g)),
      groups.join(),
    );
    assert.ok(
      groups.some((g) => /custom/i.test(g)),
      "the test machine's custom cleaner loads",
    );
  });

  it("lists only programs on this PC and marks running ones", async () => {
    assert.ok(await $('.cleaner-row[data-cleaner="chrome"]').isExisting());
    assert.ok(await $('.cleaner-row[data-cleaner="northwind"]').isExisting());
    assert.ok(
      !(await $('.cleaner-row[data-cleaner="opera"]').isExisting()),
      "Opera is not on the test machine",
    );
    const firefox = await $('.cleaner-row[data-cleaner="firefox"]');
    assert.match(await firefox.getText(), /Running/);
    assert.match(await $("#tree-count").getText(), /cleaners shown/);
    await screenshot("start");
  });
});
