import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { MemoryStatuses } from "~/features/projects/use-memory-status";
import type { Conversation, Project } from "~/lib/types";

import { listedConversations, Sidebar, SidebarItem } from "./sidebar";
import type { ConversationsState } from "./use-conversations";

vi.mock("~/lib/ipc");

const conversation = (
  id: string,
  title: string,
  projectId: string | null = null
): Conversation => ({
  id,
  title,
  model: "m",
  createdAt: "x",
  updatedAt: "x",
  settings: {
    systemPrompt: "",
    options: {},
    think: { kind: "off" },
    charts: false,
  },
  projectId,
});

const project = (id: string, name: string): Project => ({
  id,
  name,
  instructions: "",
  createdAt: "x",
  updatedAt: "x",
  autoMemory: true,
});

const noop = () => undefined;

const renderSidebar = (
  state: Partial<ConversationsState>,
  disabled = false,
  projects: Project[] = [],
  selectedProjectId: string | null = null,
  memory: MemoryStatuses = {}
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
      projects={projects}
      selectedProjectId={selectedProjectId}
      disabled={disabled}
      version="0.1.0"
      onNew={noop}
      onNewProject={() => Promise.resolve(true)}
      onOpenProject={noop}
      onSelect={noop}
      onRename={() => Promise.resolve(true)}
      onDelete={noop}
      onPrepareCopy={noop}
      onCopy={() => Promise.resolve(true)}
      memory={memory}
      onOpenActivity={noop}
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
        onPrepareCopy={noop}
        onCopy={() => Promise.resolve(true)}
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
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*aria-label="New chat"/);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>First</);
    expect(html).toMatch(/disabled=""[^>]*aria-label="Rename First"/);
    expect(html).toMatch(/disabled=""[^>]*aria-label="Delete First"/);
    // Copying only reads, so it stays available.
    expect(html).not.toMatch(/disabled=""[^>]*aria-label="Copy First/);
  });

  it("has a search box", () => {
    const html = renderSidebar({ conversations: [conversation("a", "First")] });
    expect(html).toMatch(/<input[^>]*type="search"/);
    expect(html).toContain('aria-label="Search conversations"');
    expect(html).toContain('maxLength="200"');
  });

  it("lists projects, with the open chat's group expanded", () => {
    const html = renderSidebar(
      {
        conversations: [
          conversation("a", "Loose chat"),
          conversation("b", "Thesis chat", "p1"),
          conversation("c", "Tax chat", "p2"),
        ],
        selectedId: "b",
      },
      false,
      [project("p1", "Thesis"), project("p2", "Taxes")],
      "p2"
    );
    expect(html).toContain('aria-label="New project"');
    expect(html).toMatch(
      /aria-expanded="true"[^>]*aria-label="Chats in Thesis"/
    );
    expect(html).toMatch(
      /aria-expanded="false"[^>]*aria-label="Chats in Taxes"/
    );
    expect(html).toContain('aria-label="Thesis chats"');
    expect(html).toContain("Thesis chat");
    // A collapsed group does not list its chats.
    expect(html).not.toContain("Tax chat");
    expect(html).toMatch(/aria-current="true"[^>]*>.*Taxes</);
    // The ungrouped list holds only chats outside projects.
    const ungrouped = html.slice(html.indexOf('aria-label="Conversations"'));
    expect(ungrouped).toContain("Loose chat");
    expect(ungrouped).not.toContain("Thesis chat");
  });

  it("groups the chats under date headings, newest group first", () => {
    const today = {
      ...conversation("a", "Fresh"),
      updatedAt: new Date().toISOString(),
    };
    const html = renderSidebar({
      conversations: [today, conversation("b", "Ancient")],
      selectedId: "a",
    });
    expect(html).toMatch(/>Today<\/h3><ul[^>]*>.*Fresh.*<\/ul>.*>Older</);
    expect(html).toMatch(/>Older<\/h3><ul[^>]*>.*Ancient/);
    expect(html).not.toContain("Yesterday");
    // The selected chat stands out by weight, not only its background.
    const itemClass = (title: string) =>
      new RegExp(`<li class="([^"]*)"><button[^>]*>${title}<`).exec(html)?.[1];
    expect(itemClass("Fresh")).toContain("font-medium");
    expect(itemClass("Ancient")).toBeDefined();
    expect(itemClass("Ancient")).not.toContain("font-medium");
  });

  it("shows an error", () => {
    const html = renderSidebar({
      error: { kind: "database", message: "disk full" },
    });
    expect(html).toMatch(/role="alert"[^>]*>disk full</);
  });

  it("shows a project's memory being saved and its pending count", () => {
    const html = renderSidebar(
      {},
      false,
      [project("p1", "Thesis"), project("p2", "Taxes")],
      null,
      { p1: { pending: 2, conversationId: "b", state: "running" } }
    );
    expect(html).toMatch(/Thesis.*Saving to project memory…/);
    expect(html).toContain('aria-label="2 memories pending"');
    // Only Thesis has a badge.
    expect(html.match(/aria-label="\d+ memor/g)).toHaveLength(1);
  });
});

const titles = (list: Conversation[]) => list.map(({ title }) => title);

describe("listedConversations", () => {
  const all = [
    conversation("a", "Trip plan"),
    conversation("b", "Trip budget", "p1"),
    conversation("c", "Taxes", "p1"),
  ];

  it("lists only chats outside projects without a query", () => {
    expect(titles(listedConversations(all, " ", null))).toEqual(["Trip plan"]);
  });

  it("searches every chat, in a project or not", () => {
    expect(titles(listedConversations(all, "trip", null))).toEqual([
      "Trip plan",
      "Trip budget",
    ]);
  });
});

describe("SidebarItem", () => {
  it("offers rename, copy and delete in view mode", () => {
    const html = renderItem("view");
    expect(html).toContain('aria-label="Rename Trip plan"');
    expect(html).toContain('aria-label="Copy Trip plan as Markdown"');
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
