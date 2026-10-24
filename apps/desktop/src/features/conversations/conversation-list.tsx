import type { ReactNode } from "react";
import { useId } from "react";

import { groupByDate } from "~/features/conversations/date-groups";
import type { Conversation } from "~/lib/types";

type ConversationListProps = {
  shown: Conversation[];
  /** Matches are listed flat; otherwise the chats are grouped by date. */
  searching: boolean;
  now: Date;
  item: (conversation: Conversation) => ReactNode;
};

/** The sidebar's chats under "Conversations", under date headings. */
const ConversationList = ({
  shown,
  searching,
  now,
  item,
}: ConversationListProps) => {
  const id = useId();
  if (searching) {
    return (
      <ul aria-label="Search results" className="mt-2 flex flex-col gap-0.5">
        {shown.map(item)}
      </ul>
    );
  }
  return (
    <div className="mt-2 flex flex-col gap-3">
      {groupByDate(shown, now).map(({ label, conversations }, index) => (
        <div key={label}>
          <h3
            id={`${id}-${String(index)}`}
            className="px-2 text-xs font-medium tracking-wide text-zinc-500 uppercase dark:text-zinc-400"
          >
            {label}
          </h3>
          <ul
            aria-labelledby={`${id}-${String(index)}`}
            className="mt-1 flex flex-col gap-0.5"
          >
            {conversations.map(item)}
          </ul>
        </div>
      ))}
    </div>
  );
};

export { ConversationList };
