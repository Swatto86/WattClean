/** Importing winapp2.ini adds community cleaners. */
import { strict as assert } from "node:assert";
import fs from "node:fs";
import path from "node:path";

import { dataDir, setPicks } from "../workspace.ts";
import { openPage, screenshot } from "./support.ts";

const WINAPP2 = [
  "; test copy",
  "[Northwind Logs *]",
  "LangSecRef=3021",
  String.raw`DetectFile=%LocalAppData%\Northwind`,
  String.raw`FileKey1=%LocalAppData%\Northwind\Logs|*.log|RECURSE`,
  "",
].join("\r\n");

describe("community cleaners", () => {
  it("imports winapp2.ini and lists its cleaners", async () => {
    const file = path.join(dataDir(), "imports", "winapp2.ini");
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, WINAPP2);
    setPicks({ ini: file });
    await openPage("settings");
    await $("#community-import").click();
    await browser.waitUntil(
      async () => /adds 1 cleaner/.test(await $("#community-status").getText()),
      { timeout: 15_000, timeoutMsg: "the import was not reported" },
    );
    assert.ok(
      fs.existsSync(path.join(dataDir(), "disk", "cleaners", "winapp2.ini")),
    );
    await screenshot("settings");
    await openPage("cleaner");
    await browser.waitUntil(
      async () =>
        (await $("#cleaner-tree").getText()).includes("Northwind Logs"),
      { timeout: 15_000, timeoutMsg: "the community cleaner is not listed" },
    );
  });
});
