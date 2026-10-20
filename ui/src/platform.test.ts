import { strict as assert } from "node:assert";
import { test } from "node:test";

import { samePath, setPlatform } from "./platform.ts";

test("paths differing only by case are one on Windows and macOS only", () => {
  setPlatform("windows");
  assert.ok(
    samePath(String.raw`C:\Docs\Report.txt`, String.raw`c:\docs\report.TXT`),
  );
  setPlatform("macos");
  assert.ok(samePath("/Users/t/Report.txt", "/Users/t/report.txt"));
  setPlatform("linux");
  assert.ok(!samePath("/home/t/Report.txt", "/home/t/report.txt"));
  assert.ok(samePath("/home/t/Report.txt", "/home/t/Report.txt"));
});
