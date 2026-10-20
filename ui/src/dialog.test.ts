import { strict as assert } from "node:assert";
import { test } from "node:test";

import { dialogsWork } from "./dialog.ts";

test("a web engine without <dialog>'s showModal is told apart", () => {
  assert.ok(!dialogsWork(undefined), "no HTMLDialogElement at all");
  class Unknown {}
  assert.ok(!dialogsWork(Unknown), "an element without showModal");
  class Dialog {
    showModal(): void {}
  }
  assert.ok(dialogsWork(Dialog));
});
