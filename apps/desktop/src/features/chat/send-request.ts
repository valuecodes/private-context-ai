import {
  chatEdit,
  chatRegenerate,
  chatSend,
  conversationCreate,
  tempChatDiscard,
  tempChatEdit,
  tempChatRegenerate,
  tempChatSend,
  tempChatStart,
} from "~/lib/ipc";
import type { ChatEvent, Conversation, SendStarted, Think } from "~/lib/types";

const TITLE_CHARS = 60;

/**
 * A title for a new conversation: the prompt's first non-empty line, cut to
 * 60 characters. `null` lets Rust use its default.
 */
const titleFromPrompt = (prompt: string): string | null => {
  const line = prompt
    .split("\n")
    .map((part) => part.replaceAll(/\p{Cc}/gu, " ").trim())
    .find((part) => part !== "");
  if (line === undefined) {
    return null;
  }
  // Spread splits by code point, so a cut never halves an emoji.
  return [...line].slice(0, TITLE_CHARS).join("").trim();
};

type ChatCallbacks = {
  /** The first send of a new chat saved it as this conversation. */
  onCreated: (conversation: Conversation) => void;
  /** Rust admitted a send, so the conversation was just used with `model`. */
  onStarted: (conversationId: string, model: string) => void;
};

/** Where a send goes. A temporary chat is never saved, so it has no callbacks. */
type SendTarget =
  | { kind: "saved"; conversationId: string }
  | { kind: "new" }
  | { kind: "temporary"; sessionId: Promise<string> };

/**
 * What a turn sends: a new prompt, a new reply to the last prompt, or the
 * user message `messageId` replaced by `prompt`.
 */
type TurnInput =
  | { kind: "send"; prompt: string }
  | { kind: "regenerate" }
  | { kind: "edit"; messageId: string; prompt: string };

type SendRequest = { model: string; think: Think | null; input: TurnInput };

type OnEvent = (event: ChatEvent) => void;

const sendSaved = (
  conversationId: string,
  { model, think, input }: SendRequest,
  onEvent: OnEvent
): Promise<SendStarted> => {
  if (input.kind === "regenerate") {
    return chatRegenerate({ conversationId, model, think }, onEvent);
  }
  if (input.kind === "edit") {
    const { messageId, prompt } = input;
    return chatEdit(
      { conversationId, messageId, model, prompt, think },
      onEvent
    );
  }
  return chatSend(
    { conversationId, model, prompt: input.prompt, think },
    onEvent
  );
};

const sendTemporary = (
  sessionId: string,
  { model, think, input }: SendRequest,
  onEvent: OnEvent
): Promise<SendStarted> => {
  if (input.kind === "regenerate") {
    return tempChatRegenerate({ sessionId, model, think }, onEvent);
  }
  if (input.kind === "edit") {
    const { messageId, prompt } = input;
    return tempChatEdit(
      { sessionId, messageId, model, prompt, think },
      onEvent
    );
  }
  return tempChatSend(
    { sessionId, model, prompt: input.prompt, think },
    onEvent
  );
};

/**
 * Starts the request once `ready` settles and resolves with its ids. A new
 * chat is created at once, so the sidebar shows it without waiting.
 */
const startRequest = (
  target: SendTarget,
  request: SendRequest,
  onEvent: OnEvent,
  { onCreated, onStarted }: ChatCallbacks,
  ready: Promise<void> = Promise.resolve()
): Promise<SendStarted> => {
  if (target.kind === "temporary") {
    return ready
      .then(() => target.sessionId)
      .then((sessionId) => sendTemporary(sessionId, request, onEvent));
  }
  const { input, model } = request;
  const conversationId: Promise<string> =
    target.kind === "saved"
      ? Promise.resolve(target.conversationId)
      : conversationCreate(
          titleFromPrompt(input.kind === "regenerate" ? "" : input.prompt),
          model
        ).then((created) => {
          onCreated(created);
          return created.id;
        });
  return ready
    .then(() => conversationId)
    .then((id) =>
      sendSaved(id, request, onEvent).then((started) => {
        onStarted(id, model);
        return started;
      })
    );
};

type TempSession = {
  /** The session id, starting the session on the first call. */
  id: () => Promise<string>;
  discard: () => void;
};

/**
 * The temporary chat's Rust session, started by its first send. `discard`
 * forgets it at once, so the next temporary chat gets a fresh session even
 * while this one is still being created; Rust drops it once it exists.
 */
const tempSession = (): TempSession => {
  let current: Promise<string> | null = null;
  return {
    id: () => {
      if (current === null) {
        const started = tempChatStart();
        current = started;
        // A refused start is not kept, so the next send tries again.
        started.catch(() => {
          if (current === started) {
            current = null;
          }
        });
      }
      return current;
    },
    discard: () => {
      const discarded = current;
      current = null;
      void discarded?.then(tempChatDiscard).catch(() => undefined);
    },
  };
};

export type { ChatCallbacks, SendTarget, TempSession, TurnInput };
export { startRequest, tempSession, titleFromPrompt };
