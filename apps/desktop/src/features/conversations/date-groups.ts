import type { Conversation } from "~/lib/types";

const LABELS = [
  "Today",
  "Yesterday",
  "Previous 7 days",
  "Previous 30 days",
  "Older",
] as const;

type DateLabel = (typeof LABELS)[number];

type DateGroup = { label: DateLabel; conversations: Conversation[] };

/** Local midnight `days` calendar days before `now`'s; DST-safe. */
const daysBefore = (now: Date, days: number): number =>
  new Date(now.getFullYear(), now.getMonth(), now.getDate() - days).getTime();

/**
 * Buckets conversations by `updatedAt` on local calendar days, keeping their
 * order within each group and dropping empty groups. A future time counts as
 * Today, an unreadable one as Older.
 */
const groupByDate = (conversations: Conversation[], now: Date): DateGroup[] => {
  // The earliest time of each bucket but Older, in `LABELS` order.
  const starts = [0, 1, 7, 30].map((days) => daysBefore(now, days));
  const groups = LABELS.map((label): DateGroup => ({
    label,
    conversations: [],
  }));
  for (const conversation of conversations) {
    const time = new Date(conversation.updatedAt).getTime();
    const index = starts.findIndex((start) => time >= start);
    groups[index === -1 ? starts.length : index]?.conversations.push(
      conversation
    );
  }
  return groups.filter((group) => group.conversations.length > 0);
};

export { groupByDate };
