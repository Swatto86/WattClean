// What a removal left behind, or the background jobs of apps already gone
// (macOS): a dialog lists them, nothing ticked, each can be shown in the
// file manager first, and the ticked ones go to the Trash.

import { inform } from "../dialog.ts";
import { el, notice } from "../dom.ts";
import { plural } from "../format.ts";
import { trashName } from "../platform.ts";
import { packagesApi, type LeftoverView } from "./api.ts";
import { leftoverNote, revealLabel } from "./text.ts";

/** Asks which leftovers to move to the Trash; resolves with their numbers. */
const chooseLeftovers = (
  title: string,
  intro: string,
  leftovers: LeftoverView[],
): Promise<number[]> =>
  new Promise((resolve) => {
    const boxes = leftovers.map((leftover) => {
      const box = el("input", {
        type: "checkbox",
        id: `un-pkg-left-${leftover.number}`,
      });
      return { box, leftover };
    });
    const shown = el("p", {
      class: "muted",
      id: "un-pkg-reveal-status",
      role: "status",
    });
    const show = (leftover: LeftoverView) => {
      const button = el("button", {
        type: "button",
        class: "link",
        id: `un-pkg-show-${leftover.number}`,
        text: "Show",
        title: revealLabel(),
        "aria-describedby": `un-pkg-path-${leftover.number}`,
      });
      button.addEventListener("click", () => {
        shown.textContent = "";
        packagesApi.reveal(leftover.number).catch((error: unknown) => {
          shown.textContent = String(error);
        });
      });
      return button;
    };
    const form = el(
      "form",
      { method: "dialog", class: "dialog-form" },
      el("h2", {
        class: "dialog-title",
        id: "dialog-title",
        text: title,
      }),
      el("p", { class: "dialog-text", text: intro }),
      el(
        "ul",
        { class: "list", id: "un-pkg-leftovers" },
        ...boxes.map(({ box, leftover }) =>
          el(
            "li",
            {},
            el(
              "label",
              { class: "check", for: box.id },
              box,
              el("span", {
                class: "path",
                id: `un-pkg-path-${leftover.number}`,
                text: leftover.path,
              }),
            ),
            el("span", { class: "muted", text: leftoverNote(leftover) }),
            show(leftover),
          ),
        ),
      ),
      shown,
      el(
        "div",
        { class: "dialog-buttons" },
        el("button", {
          type: "submit",
          value: "ok",
          class: "primary",
          text: `Move to the ${trashName()}`,
        }),
        el("button", {
          type: "submit",
          value: "cancel",
          class: "secondary",
          text: "Keep them",
        }),
      ),
    );
    const dialog = el(
      "dialog",
      { class: "dialog", "aria-labelledby": "dialog-title" },
      form,
    );
    let chosen: number[] = [];
    form.addEventListener("submit", (event) => {
      const submitter = event.submitter;
      if (submitter instanceof HTMLButtonElement && submitter.value === "ok") {
        chosen = boxes
          .filter(({ box }) => box.checked)
          .map(({ leftover }) => leftover.number);
      }
    });
    dialog.addEventListener("close", () => {
      dialog.remove();
      resolve(chosen);
    });
    document.body.append(dialog);
    dialog.showModal();
  });

/** Offers `leftovers` under `title`, moves the ticked ones to the Trash
 *  and says what moved; `kept` is said when none was ticked. */
export const offerLeftovers = async (
  title: string,
  intro: string,
  leftovers: LeftoverView[],
  kept: string,
): Promise<void> => {
  const numbers = await chooseLeftovers(title, intro, leftovers);
  if (numbers.length === 0) {
    notice(kept);
    return;
  }
  const trashed = await packagesApi.trashLeftovers(numbers);
  if (trashed.failed.length > 0) {
    await inform(
      "Some leftovers were not moved",
      `${plural(trashed.moved, "item")} went to the ${trashName()}; these could not:`,
      trashed.failed,
    );
  } else {
    notice(`${plural(trashed.moved, "item")} moved to the ${trashName()}.`);
  }
};
