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

/**
 * Writes text that is still on its way. Must be called before any `await`:
 * a `ClipboardItem` holding a promise keeps the click's user activation
 * while the text arrives (Chromium, WebKit).
 */
const writeLater = async (text: Promise<string>): Promise<void> => {
  if (typeof ClipboardItem === "undefined") {
    throw new Error("no ClipboardItem");
  }
  const blob = text.then((value) => new Blob([value], { type: "text/plain" }));
  await globalThis.navigator.clipboard.write([
    new ClipboardItem({ "text/plain": blob }),
  ]);
};

/**
 * Like `copyText`, for text an IPC call is still fetching; call it straight
 * from the click handler. Resolves `false` instead of throwing, also when
 * `text` rejects.
 */
const copyLater = async (text: Promise<string>): Promise<boolean> => {
  const written = writeLater(text).then(
    () => true,
    () => false
  );
  let value: string;
  try {
    value = await text;
  } catch {
    return false;
  }
  return (await written) || copyText(value);
};

export { copyLater, copyText };
