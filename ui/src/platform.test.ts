import { strict as assert } from "node:assert";
import { test } from "node:test";

import {
  runningBanner,
  runningWarning,
  samePath,
  setPlatform,
  wipeCaveat,
} from "./platform.ts";

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

test("running programs lose open files on Windows, everything elsewhere", () => {
  setPlatform("windows");
  assert.equal(
    runningWarning("Firefox"),
    "Firefox is running: files it has open will be skipped.",
  );
  assert.equal(
    runningBanner(["Firefox", "Slack"]),
    "Running now: Firefox, Slack. Close them before cleaning, or files they have open are skipped.",
  );
  setPlatform("macos");
  assert.equal(
    runningWarning("Claude"),
    "Claude is running, so it will be left alone: quit it first to clean it.",
  );
  assert.equal(
    runningBanner(["Claude"]),
    "Running now: Claude. It was left alone: quit it, then clean again.",
  );
  setPlatform("linux");
  assert.equal(
    runningBanner(["Firefox", "Slack"]),
    "Running now: Firefox, Slack. They were left alone: close them, then clean again.",
  );
});

test("a wipe of the Mac's start-up disk warns that local snapshots go", () => {
  setPlatform("macos");
  assert.match(wipeCaveat(false), /Time Machine's local snapshots.*iCloud/);
  assert.equal(wipeCaveat(true), "");
  setPlatform("windows");
  assert.equal(wipeCaveat(false), "");
  setPlatform("linux");
  assert.equal(wipeCaveat(false), "");
});
