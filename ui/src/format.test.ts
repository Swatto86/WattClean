import { strict as assert } from "node:assert";
import { test } from "node:test";

import { bytes, count, duration, percent, plural } from "./format.ts";

test("sizes read like Explorer's", () => {
  assert.equal(bytes(0), "0 bytes");
  assert.equal(bytes(1), "1 byte");
  assert.equal(bytes(1023), "1023 bytes");
  assert.equal(bytes(1024), "1.00 KB");
  assert.equal(bytes(1536), "1.50 KB");
  assert.equal(bytes(12.34 * 1024 * 1024), "12.3 MB");
  assert.equal(bytes(250 * 1024 * 1024 * 1024), "250 GB");
  assert.equal(bytes(-5), "0 bytes");
  assert.equal(bytes(Number.NaN), "0 bytes");
});

test("counts and plurals", () => {
  assert.equal(count(1234567), "1,234,567");
  assert.equal(plural(1, "file"), "1 file");
  assert.equal(plural(2, "file"), "2 files");
  assert.equal(plural(3, "entry", "entries"), "3 entries");
});

test("durations and percentages", () => {
  assert.equal(duration(850), "850 ms");
  assert.equal(duration(4200), "4.2 s");
  assert.equal(duration(125_000), "2 min 5 s");
  assert.equal(percent(1, 4), 25);
  assert.equal(percent(5, 0), 0);
  assert.equal(percent(9, 4), 100);
});
