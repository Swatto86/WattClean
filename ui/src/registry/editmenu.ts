/** What a right-click offers: Windows' edit items on text, nothing elsewhere. */

/** Each item shown enabled (true), greyed (false) or not at all (absent). */
export interface EditMenu {
  cut?: boolean;
  copy?: boolean;
  paste?: boolean;
  selectAll?: boolean;
}

/** A text field, as far as its menu is concerned. */
export interface Field {
  editable: boolean;
  /** Characters selected in it. */
  selected: number;
  /** Characters in it. */
  length: number;
}

/** Input types whose text can be cut, copied and pasted (not passwords). */
const TEXT_INPUTS = new Set(["text", "search", "url", "email", "tel"]);

/**
 * The menu for a right-click on `field`, or elsewhere (`null`) while the
 * page has `selection` selected; `null` for no menu at all.
 */
export function itemsFor(
  field: Field | null,
  selection: string,
): EditMenu | null {
  if (field) {
    const selected = field.selected > 0;
    return {
      cut: field.editable && selected,
      copy: selected,
      paste: field.editable,
      selectAll: field.length > 0,
    };
  }
  return selection === "" ? null : { copy: true };
}

/** The menu for a right-click on `target`; `null` for none. */
export function editMenuFor(
  target: EventTarget | null,
  selection: string,
): EditMenu | null {
  return itemsFor(fieldOf(target), selection);
}

/** A field whose text can be edited or copied (not a checkbox or password). */
export function isTextField(
  target: EventTarget | null,
): target is HTMLInputElement | HTMLTextAreaElement {
  return (
    target instanceof HTMLTextAreaElement ||
    (target instanceof HTMLInputElement && TEXT_INPUTS.has(target.type))
  );
}

function fieldOf(target: EventTarget | null): Field | null {
  if (!isTextField(target)) return null;
  return {
    editable: !target.readOnly && !target.disabled,
    selected: (target.selectionEnd ?? 0) - (target.selectionStart ?? 0),
    length: target.value.length,
  };
}
