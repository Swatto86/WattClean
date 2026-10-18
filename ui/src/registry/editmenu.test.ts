import { strict as assert } from "node:assert";
import { test } from "node:test";

import { itemsFor } from "./editmenu.ts";

test("an empty field offers Paste only, the rest greyed", () => {
  assert.deepEqual(itemsFor({ editable: true, selected: 0, length: 0 }, ""), {
    cut: false,
    copy: false,
    paste: true,
    selectAll: false,
  });
});

test("selected text in a field can be cut and copied", () => {
  assert.deepEqual(
    itemsFor({ editable: true, selected: 4, length: 10 }, "ignored"),
    { cut: true, copy: true, paste: true, selectAll: true },
  );
});

test("a read-only field can be copied from but not changed", () => {
  assert.deepEqual(itemsFor({ editable: false, selected: 4, length: 10 }, ""), {
    cut: false,
    copy: true,
    paste: false,
    selectAll: true,
  });
});

test("outside fields only selected text has a menu, with Copy alone", () => {
  assert.deepEqual(itemsFor(null, "C:\\Program Files\\Go"), { copy: true });
  assert.equal(itemsFor(null, ""), null);
});
