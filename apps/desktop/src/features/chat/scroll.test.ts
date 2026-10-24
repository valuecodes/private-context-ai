import { describe, expect, it } from "vitest";

import { followsAfterScroll, isAtBottom } from "./scroll";

describe("isAtBottom", () => {
  it("counts less than 32 px from the end as the bottom", () => {
    // 1000 tall, 500 shown: the end is at scrollTop 500.
    expect(isAtBottom(1000, 500, 500)).toBe(true);
    expect(isAtBottom(1000, 468.5, 500)).toBe(true);
    expect(isAtBottom(1000, 468, 500)).toBe(false);
    expect(isAtBottom(1000, 0, 500)).toBe(false);
  });
});

describe("followsAfterScroll", () => {
  const step = {
    atBottom: false,
    jumping: false,
    scrollTop: 100,
    lastScrollTop: 50,
  };

  it("follows at the bottom", () => {
    expect(followsAfterScroll({ ...step, atBottom: true })).toBe(true);
  });

  it("keeps following while a jump moves down", () => {
    expect(followsAfterScroll({ ...step, jumping: true })).toBe(true);
  });

  it("stops following when the user scrolls up, even mid-jump", () => {
    expect(followsAfterScroll({ ...step, jumping: true, scrollTop: 40 })).toBe(
      false
    );
  });

  it("stops following above the bottom without a jump", () => {
    expect(followsAfterScroll(step)).toBe(false);
  });
});
