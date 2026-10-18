/** A program whose uninstaller is missing can still be cleaned up. */
import { strict as assert } from "node:assert";

import {
  leftoverPaths,
  ready,
  selectProgram,
  waitForButton,
} from "./support.ts";

describe("a missing uninstaller", () => {
  it("says so and offers the leftover scan anyway", async () => {
    await ready();
    await selectProgram("Northwind Broken Tool");
    await $("#un-uninstall").click();
    await $("#un-mode-safe").click();
    await $("#un-wizard-restore").click();
    await $("#un-wizard-start").click();

    const anyway = await waitForButton("wizard-scan-anyway");
    assert.match(
      await $("#un-wizard-steps").getText(),
      /uninstaller is missing/,
    );
    await anyway.click();

    const paths = await leftoverPaths();
    assert.ok(
      // Its publisher folder held only this program, so it goes whole.
      paths.some((p) => p.endsWith("Program Files\\Northwind")),
      paths.join("\n"),
    );
    assert.ok(
      paths.some((p) =>
        p.startsWith(
          "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\BrokenTool",
        ),
      ),
    );
    await (await waitForButton("wizard-skip")).click();
    assert.equal(
      await $("#un-wizard-result").getText(),
      "Leftovers were kept.",
    );
    await (await waitForButton("wizard-close")).click();
    await $(".dialog").waitForExist({ reverse: true, timeout: 5_000 });
  });
});
