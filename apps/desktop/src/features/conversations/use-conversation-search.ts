import { useEffect, useRef, useState } from "react";

import { conversationSearch } from "~/lib/ipc";
import type { Conversation } from "~/lib/types";

/** How long typing pauses before message content is searched. */
const SEARCH_DEBOUNCE_MS = 200;

/** The conversations whose messages matched `query`, from Rust. */
type ContentMatches = { query: string; ids: ReadonlySet<string> };

/**
 * Title matches at once, plus content matches once Rust has answered for
 * this very query: ids found for an earlier query are never shown for a new
 * one. An empty query keeps the whole list.
 */
const filterConversations = (
  conversations: Conversation[],
  query: string,
  matches: ContentMatches | null
): Conversation[] => {
  const wanted = query.trim();
  if (wanted === "") {
    return conversations;
  }
  const lower = wanted.toLocaleLowerCase();
  const ids = matches?.query === wanted ? matches.ids : null;
  return conversations.filter(
    ({ id, title }) =>
      title.toLocaleLowerCase().includes(lower) || ids?.has(id) === true
  );
};

/**
 * Searches message content for `query` after a pause in typing, and again
 * whenever `conversations` changes (a send moves its chat to the top) or a
 * saved reply stops `streaming`, so its saved text is searched. One search
 * runs at a time, so typing never queues work on the database: a query
 * typed meanwhile waits, and only the newest one runs next. A failed search
 * finds nothing, so the title matches still show.
 */
const useConversationSearch = (
  query: string,
  conversations: Conversation[],
  streaming: boolean
): ContentMatches | null => {
  const wanted = query.trim();
  const [matches, setMatches] = useState<ContentMatches | null>(null);
  const latest = useRef(wanted);
  const running = useRef(false);
  const queued = useRef<string | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    latest.current = wanted;
    if (wanted === "") {
      return undefined;
    }
    const run = (text: string) => {
      running.current = true;
      const found = (ids: string[]) => {
        running.current = false;
        if (!mounted.current) {
          return;
        }
        if (text === latest.current) {
          setMatches({ query: text, ids: new Set(ids) });
        }
        const next = queued.current;
        queued.current = null;
        if (next !== null && next === latest.current) {
          run(next);
        }
      };
      void conversationSearch(text).then(found, () => {
        found([]);
      });
    };
    const timer = setTimeout(() => {
      if (running.current) {
        queued.current = wanted;
      } else {
        run(wanted);
      }
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [wanted, conversations, streaming]);

  return wanted === "" ? null : matches;
};

export { filterConversations, useConversationSearch };
