import { strict as assert } from "node:assert";
import { test } from "node:test";

import { setPlatform } from "../platform.ts";
import { removalHow, revealLabel } from "./text.ts";

test("Homebrew's password box and service stop are promised on macOS only", () => {
  setPlatform("macos");
  assert.equal(
    removalHow("Homebrew"),
    "Homebrew removes it, first stopping its background service if it runs as one. If Homebrew needs an administrator's password, WattClean asks for it in a password box.",
  );
  assert.equal(
    removalHow("Homebrew Cask"),
    "Homebrew Cask removes it. If Homebrew needs an administrator's password, WattClean asks for it in a password box.",
  );
  assert.equal(revealLabel(), "Show in Finder");
  setPlatform("linux");
  assert.equal(removalHow("Homebrew"), "Homebrew removes it.");
  assert.equal(
    removalHow("APT"),
    "APT removes it, asking for an administrator's password when it needs one.",
  );
  assert.equal(removalHow("AppImage"), "The file moves to the Trash.");
  assert.equal(revealLabel(), "Open the folder holding it");
});
