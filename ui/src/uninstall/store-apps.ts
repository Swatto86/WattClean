/** Windows owns Store package removal; never offer a registry leftover scan. */
import { api } from "./api.ts";
import { Modal } from "./dialog.ts";
import { h } from "./dom.ts";
import { plural } from "./format.ts";
import type { StoreApp } from "./schema-store.ts";

const message = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

export async function showStoreApps(): Promise<void> {
  const modal = new Modal("Windows Store apps", true);
  let closed = false;
  let apps: StoreApp[] = [];
  let query = "";
  const close = () => {
    closed = true;
    modal.close();
  };
  const busy = (text: string) => {
    modal.dismissWith(null);
    modal.show(
      "Windows Store apps",
      [h("p", { role: "status" }, text)],
      [],
      () => undefined,
    );
  };
  const load = async (notice = "") => {
    busy("Reading Store apps for this Windows account…");
    try {
      apps = await api.listStoreApps();
      if (!closed) render(notice);
    } catch (error: unknown) {
      if (!closed)
        render(`${notice} Could not refresh the list: ${message(error)}`);
    }
  };
  const remove = async (app: StoreApp) => {
    busy(`Removing ${app.name} for this Windows account…`);
    let notice: string;
    try {
      await api.removeStoreApp(app.id);
      notice = `${app.name} was removed.`;
    } catch (error: unknown) {
      notice = message(error);
    }
    await load(notice);
  };
  const confirmRemoval = (app: StoreApp) => {
    modal.dismissWith(() => render());
    modal.show(
      `Uninstall ${app.name}?`,
      [
        h(
          "p",
          {},
          "Windows will remove this app and its app data for the current account only. Other accounts and provisioned apps are unchanged. WattClean cannot back up or restore Store app data; reinstall the app from Microsoft Store if needed.",
        ),
        h("p", { class: "path" }, app.id),
      ],
      [
        { label: "Cancel", value: "cancel" },
        {
          label: "Uninstall",
          value: "remove",
          kind: "danger",
          id: "store-confirm",
        },
      ],
      (value) => {
        if (value === "remove") void remove(app);
        else render();
      },
    );
  };
  const render = (notice = "") => {
    const search = h("input", {
      id: "store-search",
      type: "search",
      "aria-label": "Search Store apps",
      placeholder: "Search Store apps",
    });
    search.value = query;
    const rows = h("tbody", { id: "store-rows" });
    const count = h("p", { id: "store-count", role: "status" });
    const filter = () => {
      const needle = query.trim().toLowerCase();
      const shown = apps.filter((app) =>
        `${app.name} ${app.publisher} ${app.id}`.toLowerCase().includes(needle),
      );
      count.textContent = plural(shown.length, "Store app");
      rows.replaceChildren(
        ...shown.map((app) => {
          const button = h(
            "button",
            {
              type: "button",
              class: "ghost",
              "aria-label": `Uninstall ${app.name}`,
            },
            "Uninstall",
          );
          button.addEventListener("click", () => confirmRemoval(app));
          return h(
            "tr",
            { "data-package": app.id },
            h(
              "td",
              {},
              h("strong", {}, app.name),
              h("div", { class: "path" }, app.id),
            ),
            h("td", {}, app.version),
            h("td", {}, button),
          );
        }),
      );
    };
    search.addEventListener("input", () => {
      query = search.value;
      filter();
    });
    filter();
    modal.dismissWith(close);
    modal.show(
      "Windows Store apps",
      [
        h(
          "p",
          {},
          "Apps installed for this Windows account. Protected Windows components and shared frameworks are excluded.",
        ),
        h("p", { id: "store-notice", role: "status" }, notice),
        search,
        count,
        h(
          "div",
          { class: "store-table" },
          h(
            "table",
            {},
            h(
              "thead",
              {},
              h(
                "tr",
                {},
                h("th", { scope: "col" }, "App / package"),
                h("th", { scope: "col" }, "Version"),
                h("th", { scope: "col" }, "Action"),
              ),
            ),
            rows,
          ),
        ),
      ],
      [
        { label: "Refresh", value: "refresh" },
        { label: "Close", value: "close" },
      ],
      (value) => {
        if (value === "refresh") void load();
        else close();
      },
    );
  };
  await load();
}
