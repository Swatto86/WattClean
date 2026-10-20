import { strict as assert } from "node:assert";
import { test } from "node:test";

import { setPlatform } from "../platform.ts";
import type { LeftoverView } from "./api.ts";
import {
  leftoverNote,
  leftoversIntro,
  orphansIntro,
  removalHow,
  revealLabel,
} from "./text.ts";

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

const leftover = (
  job: boolean,
  systemWide: boolean,
  size: number | null,
): LeftoverView => ({
  number: 0,
  path: "/Library/LaunchDaemons/x.plist",
  size,
  folder: false,
  job,
  systemWide,
});

test("a leftover says whether it is a background job and for every account", () => {
  assert.equal(leftoverNote(leftover(false, false, null)), "size unknown");
  assert.equal(
    leftoverNote(leftover(true, false, 371)),
    "background job, 371 bytes",
  );
  assert.equal(
    leftoverNote(leftover(true, true, 686)),
    "background job, for every account, 686 bytes",
  );
});

test("the password for items in /Library is promised only when one is listed", () => {
  setPlatform("macos");
  const home = [leftover(true, false, 1)];
  const both = [...home, leftover(true, true, 1)];
  const asks =
    " Items for every account ask for an administrator's password when they move.";
  assert.ok(!leftoversIntro("Tea", home).includes("password"));
  assert.ok(leftoversIntro("Tea", both).endsWith(asks));
  assert.ok(
    leftoversIntro("Tea", both).startsWith(
      "Tick what to move to the Trash. Only items named after Tea",
    ),
  );
  assert.ok(!orphansIntro(home).includes("password"));
  assert.ok(orphansIntro(both).endsWith(asks));
  assert.ok(orphansIntro(home).includes("apps that are no longer on this Mac"));
});
