// In-app dialogs on the native <dialog> element: it traps focus, closes on
// Escape and returns focus where it was. Each resolves with the answer, or
// null when cancelled.

import { el } from "./dom.ts";

interface ConfirmOptions {
  title: string;
  message: string;
  ok: string;
  danger?: boolean;
  /** Lines shown as a list under the message. */
  list?: string[];
  /** A box such as "Don't ask again". */
  checkbox?: string;
  /** No Cancel button: the dialog only informs. */
  alone?: boolean;
}

/** Asks a question; resolves with the box's state, or null when cancelled. */
export const confirm = (
  options: ConfirmOptions,
): Promise<{ checked: boolean } | null> =>
  new Promise((resolve) => {
    const form = el("form", { method: "dialog", class: "dialog-form" });
    form.append(
      el("h2", {
        class: "dialog-title",
        id: "dialog-title",
        text: options.title,
      }),
      el("p", { class: "dialog-text", text: options.message }),
    );
    if (options.list?.length) {
      form.append(
        el(
          "ul",
          { class: "dialog-list" },
          ...options.list.map((line) => el("li", { text: line })),
        ),
      );
    }
    const box = el("input", { type: "checkbox", name: "box" });
    if (options.checkbox) {
      form.append(
        el(
          "label",
          { class: "check" },
          box,
          el("span", { text: options.checkbox }),
        ),
      );
    }
    const ok = el("button", {
      type: "submit",
      value: "ok",
      class: options.danger ? "primary danger" : "primary",
      text: options.ok,
    });
    const buttons = el("div", { class: "dialog-buttons" }, ok);
    if (!options.alone) {
      buttons.append(
        el("button", {
          type: "submit",
          value: "cancel",
          class: "secondary",
          text: "Cancel",
        }),
      );
    }
    form.append(buttons);
    const dialog = el(
      "dialog",
      { class: "dialog", "aria-labelledby": "dialog-title" },
      form,
    );
    let answer: { checked: boolean } | null = null;
    form.addEventListener("submit", (event) => {
      const submitter = event.submitter;
      if (submitter instanceof HTMLButtonElement && submitter.value === "ok") {
        answer = { checked: box.checked };
      }
    });
    dialog.addEventListener("close", () => {
      dialog.remove();
      resolve(answer);
    });
    document.body.append(dialog);
    dialog.showModal();
    ok.focus();
  });

/** Tells the person something; resolves when they close it. */
export const inform = async (
  title: string,
  message: string,
  list?: string[],
): Promise<void> => {
  await confirm({
    title,
    message,
    ok: "OK",
    alone: true,
    ...(list ? { list } : {}),
  });
};
