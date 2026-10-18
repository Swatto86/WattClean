import { strict as assert } from "node:assert";
import { test } from "node:test";

import {
  cleanerState,
  defaults,
  everything,
  key,
  matches,
  runnable,
  toggleCleaner,
  type CleanerLike,
} from "./selection.ts";

const chrome: CleanerLike = {
  id: "chrome",
  name: "Google Chrome",
  description: "Web browser",
  present: true,
  options: [
    { id: "cache", isDefault: true, warning: null },
    { id: "history", isDefault: false, warning: null },
    { id: "passwords", isDefault: false, warning: "Saved passwords go." },
  ],
};

const absent: CleanerLike = {
  id: "opera",
  name: "Opera",
  description: "Web browser",
  present: false,
  options: [{ id: "cache", isDefault: true, warning: null }],
};

test("a cleaner's box reads off, mixed or on", () => {
  assert.equal(cleanerState(chrome, new Set()), "off");
  assert.equal(cleanerState(chrome, new Set(["chrome.cache"])), "mixed");
  assert.equal(
    cleanerState(
      chrome,
      new Set(["chrome.cache", "chrome.history", "chrome.passwords"]),
    ),
    "on",
  );
});

test("ticking a cleaner skips options with warnings; unticking clears all", () => {
  const ticked = toggleCleaner(chrome, new Set());
  assert.deepEqual([...ticked].sort(), ["chrome.cache", "chrome.history"]);
  assert.deepEqual([...toggleCleaner(chrome, ticked)], []);
});

test("defaults, search and what may run", () => {
  assert.deepEqual(
    [...defaults([chrome, absent])],
    ["chrome.cache", "opera.cache"],
  );
  assert.ok(matches(chrome, "chro", []));
  assert.ok(matches(chrome, "PASS", ["Saved passwords"]));
  assert.ok(!matches(chrome, "firefox", ["Cache"]));
  const ticked = new Set([key("chrome", "cache"), key("opera", "cache")]);
  assert.deepEqual(runnable([chrome, absent], ticked, true), ["chrome.cache"]);
  assert.deepEqual(runnable([chrome, absent], ticked, false), [
    "chrome.cache",
    "opera.cache",
  ]);
});

test("everything is every option of the cleaners shown, warnings included", () => {
  assert.deepEqual(everything([chrome, absent], true), [
    "chrome.cache",
    "chrome.history",
    "chrome.passwords",
  ]);
  assert.equal(everything([chrome, absent], false).length, 4);
});
