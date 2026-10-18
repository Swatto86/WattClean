/** Every page of the suite opens from the navigation, alone on screen. */
import { strict as assert } from "node:assert";

import { openPage, ready, screenshot } from "./support.ts";

const PAGES: [string, string][] = [
  ["cleaner", "Disk cleaner"],
  ["shred", "Shred files"],
  ["wipe", "Wipe free space"],
  ["history", "Clean history"],
  ["registry", "Registry cleaner"],
  ["regsearch", "Registry search"],
  ["tuner", "Registry tuner"],
  ["watcher", "Registry watcher"],
  ["regbackups", "Registry backups"],
  ["schedules", "Schedules"],
  ["programs", "Uninstall programs"],
  ["traced", "Traced installs"],
  ["forced", "Forced uninstall"],
  ["unbackups", "Uninstall backups"],
  ["unhistory", "Uninstall history"],
  ["settings", "Settings"],
];

describe("the suite", () => {
  before(async () => {
    await ready();
  });

  it("groups the navigation by tool", async () => {
    const groups: string[] = [];
    for (const heading of await $$("#nav .nav-group")) {
      groups.push(await heading.getText());
    }
    assert.deepEqual(groups, ["DISK", "REGISTRY", "PROGRAMS"]);
  });

  for (const [id, title] of PAGES) {
    it(`opens ${title}`, async () => {
      await $(`#nav-${id}`).click();
      await browser.waitUntil(
        async () => (await $("#page-title").getText()) === title,
        { timeout: 5_000, timeoutMsg: `${title} never became the title` },
      );
      const shown = await browser.execute(
        () =>
          Array.from(
            document.querySelectorAll<HTMLElement>("#content > *"),
          ).filter((page) => !page.hidden).length,
      );
      assert.equal(shown, 1, "one page at a time");
      assert.equal(await $(`#nav-${id}`).getAttribute("aria-current"), "page");
      await screenshot(`page-${id}`);
    });
  }

  after(async () => {
    await openPage("cleaner");
  });
});
