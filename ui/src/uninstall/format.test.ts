import { strict as assert } from "node:assert";
import { test } from "node:test";

import type { Program } from "./api.ts";
import {
  bytes,
  countsText,
  initial,
  kilobytes,
  plural,
  target,
} from "./format.ts";
import { matches, range, step, view } from "./listing.ts";

test("bytes scale to readable units", () => {
  assert.equal(bytes(512), "512 B");
  assert.equal(bytes(1536), "1.5 KB");
  assert.equal(bytes(20 * 1024 * 1024), "20.0 MB");
  assert.equal(bytes(300 * 1024 * 1024), "300 MB");
  assert.equal(kilobytes(null), "");
  assert.equal(kilobytes(2048), "2.0 MB");
});

test("targets read the way Windows writes them", () => {
  assert.equal(
    target({ kind: "key", key: { hive: "HKLM", path: "SOFTWARE\\Acme" } }),
    "HKLM\\SOFTWARE\\Acme",
  );
  assert.equal(
    target({ kind: "value", key: { hive: "HKCU", path: "Run" }, name: "" }),
    "HKCU\\Run → (Default)",
  );
  assert.equal(target({ kind: "file", path: "C:\\a.lnk" }), "C:\\a.lnk");
});

test("counts and plurals", () => {
  assert.equal(plural(1, "file"), "1 file");
  assert.equal(plural(2, "registry key"), "2 registry keys");
  assert.equal(
    countsText({ keys: 2, values: 0, folders: 1, files: 0 }),
    "2 registry keys, 1 folder",
  );
  assert.equal(
    countsText({ keys: 0, values: 0, folders: 0, files: 0 }),
    "nothing",
  );
  assert.equal(initial("7-Zip"), "7");
  assert.equal(initial("  émile"), "É");
});

function program(
  name: string,
  publisher: string | null,
  sizeKb: number | null,
  installDate: string | null,
): Program {
  return {
    id: name,
    name,
    version: null,
    publisher,
    installDate,
    sizeKb,
    installLocation: null,
    uninstallCommand: null,
    website: null,
    comments: null,
    registryKey: { hive: "HKLM", path: name },
    scope: "machine64",
    msiProductCode: null,
    systemComponent: false,
  };
}

test("filtering matches every word against name and publisher", () => {
  const acme = program("Acme Widget", "Acme Software", 10, "2026-01-01");
  assert.ok(matches(acme, "widget"));
  assert.ok(matches(acme, "acme soft"));
  assert.ok(!matches(acme, "widget pro"));
  assert.ok(matches(acme, "  "));
});

test("sorting by size and date with name as tie-break", () => {
  const list = [
    program("b", null, 20, "2026-01-02"),
    program("a", null, 20, "2025-01-01"),
    program("c", null, null, null),
  ];
  assert.deepEqual(
    view(list, "", { key: "size", ascending: false }).map((p) => p.name),
    ["a", "b", "c"],
  );
  assert.deepEqual(
    view(list, "", { key: "date", ascending: true }).map((p) => p.name),
    ["c", "a", "b"],
  );
  assert.deepEqual(
    view(list, "", { key: "name", ascending: true }).map((p) => p.name),
    ["a", "b", "c"],
  );
});

test("keyboard steps stay inside the list", () => {
  assert.equal(step(0, 5, "ArrowUp"), 0);
  assert.equal(step(4, 5, "ArrowDown"), 4);
  assert.equal(step(2, 5, "End"), 4);
  assert.equal(step(2, 30, "PageDown"), 12);
  assert.equal(step(-1, 0, "ArrowDown"), -1);
});

test("Shift+click selects from the anchor to the row, either way", () => {
  const ids = ["a", "b", "c", "d"];
  assert.deepEqual(range(ids, "b", "d"), ["b", "c", "d"]);
  assert.deepEqual(range(ids, "d", "b"), ["b", "c", "d"]);
  assert.deepEqual(range(ids, "c", "c"), ["c"]);
  assert.deepEqual(range(ids, null, "c"), ["c"]);
  assert.deepEqual(range(ids, "gone", "a"), ["a"]);
  assert.deepEqual(range(ids, "a", "gone"), []);
});
