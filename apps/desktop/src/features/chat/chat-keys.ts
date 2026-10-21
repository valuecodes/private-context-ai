/** The parts of a `KeyboardEvent` the chat shortcuts look at. */
type ChatKey = {
  key: string;
  shiftKey: boolean;
  /** True while an IME composition is open; its Enter and Esc are its own. */
  isComposing: boolean;
};

/** Enter sends; Shift+Enter is left to insert a newline. */
const sendsMessage = ({ key, shiftKey, isComposing }: ChatKey): boolean =>
  key === "Enter" && !shiftKey && !isComposing;

type StopContext = {
  streaming: boolean;
  /** Focus is in an `<input>`, such as the sidebar rename field, where Esc
   * already means "cancel". */
  inTextField: boolean;
};

/** Esc stops a streaming reply, wherever focus is in the chat. */
const stopsReply = (
  { key, isComposing }: ChatKey,
  { streaming, inTextField }: StopContext
): boolean => key === "Escape" && streaming && !isComposing && !inTextField;

export { sendsMessage, stopsReply };
