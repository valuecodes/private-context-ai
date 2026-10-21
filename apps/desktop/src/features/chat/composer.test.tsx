import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { Composer } from "./composer";

const renderComposer = (streaming: boolean, canSend = true): string =>
  renderToString(
    <Composer
      streaming={streaming}
      canSend={canSend}
      onSend={() => true}
      onStop={() => undefined}
    />
  );

describe("Composer", () => {
  it("renders Send as aria-disabled, not disabled, when it cannot send", () => {
    // The draft starts empty, so Send cannot send yet.
    const html = renderComposer(false);
    expect(html).toMatch(/<button type="submit"[^>]*aria-disabled="true"/);
    expect(html).toMatch(/>Send<\/button>/);
    expect(html).not.toContain('disabled=""');
  });

  it("renders the same button as Stop with its Escape shortcut while streaming", () => {
    const html = renderComposer(true);
    expect(html).toMatch(
      /<button type="button"[^>]*aria-keyshortcuts="Escape"[^>]*>Stop<\/button>/
    );
    expect(html).not.toContain("aria-disabled");
    expect(html.match(/<button/g)).toHaveLength(1);
  });
});
