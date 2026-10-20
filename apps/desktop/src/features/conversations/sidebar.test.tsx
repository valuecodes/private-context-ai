import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { Conversation } from "~/lib/types";

import { Sidebar, SidebarItem } from "./sidebar";
import type { ConversationsState } from "./use-conversations";

vi.mock("~/lib/ipc");

const conversation = (id: string, title: string): Conversation => ({
  id,
  title,
  model: "m",
  createdAt: "x",
  updatedAt: "x",
});

const noop = () => undefined;

const renderSidebar = (
  state: Partial<ConversationsState>,
  disabled = false
): string =>
  renderToString(
    <Sidebar
      state={{
        phase: "ready",
        conversations: [],
        selectedId: null,
        error: null,
        ...state,
      }}
      disabled={disabled}
      version="0.1.0"
      onNew={noop}
      onSelect={noop}
      onRename={() => Promise.resolve(true)}
      onDelete={noop}
    />
  );

const renderItem = (mode: "view" | "rename" | "confirm", disabled = false) =>
  renderToString(
    <ul>
      <SidebarItem
        conversation={conversation("a", "Trip plan")}
        selected={false}
        disabled={disabled}
        mode={mode}
        onSelect={noop}
        onMode={noop}
        onRename={noop}
        onDelete={noop}
      />
    </ul>
  );

describe("Sidebar", () => {
  it("says when there are no conversations", () => {
    expect(renderSidebar({})).toContain("No conversations yet");
    expect(renderSidebar({ phase: "loading" })).toContain("Loading…");
  });

  it("lists titles and marks the selected one", () => {
    const html = renderSidebar({
      conversations: [conversation("a", "First"), conversation("b", "Second")],
      selectedId: "b",
    });
    expect(html).toContain("First");
    expect(html).toMatch(/aria-current="true"[^>]*>Second</);
    expect(html).not.toMatch(/aria-current="true"[^>]*>First</);
    expect(html).toContain("v0.1.0");
  });

  it("disables switching and editing while a reply streams", () => {
    const html = renderSidebar(
      { conversations: [conversation("a", "First")] },
      true
    );
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>New chat</);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>First</);
    expect(html).toMatch(/disabled=""[^>]*aria-label="Rename First"/);
    expect(html).toMatch(/disabled=""[^>]*aria-label="Delete First"/);
  });

  it("shows an error", () => {
    const html = renderSidebar({
      error: { kind: "database", message: "disk full" },
    });
    expect(html).toMatch(/role="alert"[^>]*>disk full</);
  });
});

describe("SidebarItem", () => {
  it("offers rename and delete in view mode", () => {
    const html = renderItem("view");
    expect(html).toContain('aria-label="Rename Trip plan"');
    expect(html).toContain('aria-label="Delete Trip plan"');
  });

  it("renders an input with the current title in rename mode", () => {
    const html = renderItem("rename");
    expect(html).toMatch(/<input[^>]*name="title"[^>]*value="Trip plan"/);
    expect(html).toMatch(/<button type="submit"[^>]*>Save</);
    expect(html).toContain(">Cancel<");
  });

  it("asks before deleting in confirm mode", () => {
    const html = renderItem("confirm");
    expect(html).toContain("Delete “Trip plan”?");
    expect(html).toContain(">Delete<");
    expect(html).toContain(">Keep<");
    expect(html).not.toMatch(/disabled=""[^>]*>Delete</);
  });

  it("disables a pending delete once a reply streams", () => {
    expect(renderItem("confirm", true)).toMatch(/disabled=""[^>]*>Delete</);
  });
});
