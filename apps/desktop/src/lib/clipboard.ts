/**
 * Copies with the hidden-textarea fallback for webviews without
 * `navigator.clipboard` access, such as WebKitGTK.
 */
const copyWithTextarea = (text: string): boolean => {
  if (typeof document === "undefined") {
    return false;
  }
  // `select()` moves the focus; give it back so a keyboard user stays put.
  // HTML and SVG elements both have `focus()`.
  const focused = document.activeElement as HTMLElement | null;
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.className = "fixed -left-[9999px] opacity-0";
  document.body.append(textarea);
  textarea.select();
  try {
    // Deprecated, but the only synchronous copy WebKitGTK allows.
    // oxlint-disable-next-line typescript/no-deprecated
    return document.execCommand("copy");
  } catch {
    return false;
  } finally {
    textarea.remove();
    focused?.focus();
  }
};

/** Puts `text` on the clipboard. Resolves `false` instead of throwing. */
const copyText = async (text: string): Promise<boolean> => {
  try {
    await globalThis.navigator.clipboard.writeText(text);
    return true;
  } catch {
    return copyWithTextarea(text);
  }
};

export { copyText };
