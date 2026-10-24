/** Closer than this to the end counts as at the bottom. */
const BOTTOM_THRESHOLD = 32;

const isAtBottom = (
  scrollHeight: number,
  scrollTop: number,
  clientHeight: number
): boolean => scrollHeight - scrollTop - clientHeight < BOTTOM_THRESHOLD;

type ScrollStep = {
  atBottom: boolean;
  /** A jump to the newest message is under way. */
  jumping: boolean;
  scrollTop: number;
  lastScrollTop: number;
};

/**
 * Whether new tokens are still followed after a scroll event. A smooth jump
 * passes through positions above the bottom on its way down; those keep
 * following, while any move up hands control back to the user.
 */
const followsAfterScroll = ({
  atBottom,
  jumping,
  scrollTop,
  lastScrollTop,
}: ScrollStep): boolean => atBottom || (jumping && scrollTop >= lastScrollTop);

export { followsAfterScroll, isAtBottom };
