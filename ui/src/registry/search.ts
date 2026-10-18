/** The Search view: find keys, values and data, remove what is chosen. */
import { api, message } from "./api.ts";
import { follow, showNotes } from "./cleaner.ts";
import { cleanTicked } from "./clean.ts";
import { exclude, type AppContext } from "./context.ts";
import { byId } from "./dom.ts";
import { count } from "./format.ts";
import { FindingsTree } from "./results.ts";

export function wireSearch(context: AppContext): void {
  const form = byId("search-form", HTMLFormElement);
  const error = byId("search-error", HTMLParagraphElement);
  const go = byId("search-go", HTMLButtonElement);
  const stop = byId("search-cancel", HTMLButtonElement);
  const results = byId("search-results", HTMLDivElement);
  const toolbar = byId("search-toolbar", HTMLDivElement);
  const remove = byId("search-delete", HTMLButtonElement);
  const summary = byId("search-summary", HTMLParagraphElement);
  const checked = (id: string) => byId(id, HTMLInputElement).checked;

  const tree = new FindingsTree({
    root: byId("search-tree", HTMLDivElement),
    list: "search",
    titles: context.titles,
    elevated: context.info.elevated,
    exclude: (key) => exclude(context, key),
    changed: () => {
      const ticked = tree.tickedIds().length;
      remove.textContent = `Remove (${ticked})`;
      remove.disabled = ticked === 0;
      summary.textContent =
        tree.count() === 0
          ? "Nothing matches."
          : `${count(tree.count(), "match", "matches")}, ${ticked} ticked. Tick what to remove.`;
    },
  });

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    error.textContent = "";
    const request = {
      text: byId("search-text", HTMLInputElement).value,
      within: byId("search-within", HTMLInputElement).value,
      keys: checked("search-keys"),
      values: checked("search-values"),
      data: checked("search-data"),
      matchCase: checked("search-case"),
      whole: checked("search-whole"),
    };
    void (async () => {
      go.disabled = true;
      stop.hidden = false;
      try {
        const search = api.searchRegistry(request);
        await follow(byId("search-phase", HTMLSpanElement), search);
        const report = await search;
        showNotes(byId("search-notes", HTMLDivElement), report);
        tree.show(report.findings);
        results.hidden = false;
        toolbar.hidden = false;
      } catch (failure: unknown) {
        error.textContent = message(failure);
      } finally {
        go.disabled = false;
        stop.hidden = true;
        byId("search-phase", HTMLSpanElement).textContent = "";
      }
    })();
  });
  stop.addEventListener("click", () => void api.cancelScan());
  remove.addEventListener(
    "click",
    () => void cleanTicked(context, "search", tree),
  );
}
