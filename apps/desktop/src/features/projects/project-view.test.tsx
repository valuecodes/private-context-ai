import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { Conversation, Project } from "~/lib/types";

import { deleteQuestion, ProjectView } from "./project-view";

vi.mock("~/lib/ipc");

const project: Project = {
  id: "p1",
  name: "Thesis",
  instructions: "Cite sources.",
  createdAt: "x",
  updatedAt: "x",
  autoMemory: true,
};

const chat = (id: string, title: string): Conversation => ({
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
  projectId: "p1",
});

const view = (
  chats: Conversation[],
  disabled = false,
  canStartTemporary = true
) =>
  renderToString(
    <ProjectView
      project={project}
      chats={chats}
      disabled={disabled}
      canStartTemporary={canStartTemporary}
      error={null}
      onRename={() => Promise.resolve(true)}
      onInstructions={vi.fn()}
      onAutoMemory={vi.fn()}
      onNewChat={vi.fn()}
      onNewTemporary={vi.fn()}
      onOpenChat={vi.fn()}
      onDelete={vi.fn()}
    />
  );

describe("ProjectView", () => {
  it("shows the name, instructions and chats", () => {
    const html = view([chat("a", "Chapter one"), chat("b", "Sources")]);
    expect(html).toMatch(/<h2[^>]*>Thesis</);
    expect(html).toMatch(
      /<textarea[^>]*id="project-instructions"[^>]*>Cite sources.<\/textarea>/
    );
    expect(html).toContain('aria-label="Project chats"');
    expect(html).toContain("Chapter one");
    expect(html).toContain(">New chat<");
    expect(html).toContain(">New temporary chat<");
    expect(html).toContain('aria-label="Rename Thesis"');
  });

  it("says when the project has no chats", () => {
    expect(view([])).toContain("No chats in this project yet");
  });

  it("asks before deleting, with the saved chat count", () => {
    expect(deleteQuestion("Thesis", 3)).toBe(
      "Delete “Thesis” and its 3 saved chats? An open temporary chat in it is closed too."
    );
    expect(deleteQuestion("Thesis", 1)).toContain("its 1 saved chat?");
  });

  it("starts a temporary chat only while no reply runs", () => {
    expect(view([])).not.toMatch(/disabled=""[^>]*>.*New temporary chat</);
    expect(view([], false, false)).toMatch(
      /<button[^>]*disabled=""[^>]*>.*New temporary chat</
    );
    // A new saved chat can still start: it discards the temporary one.
    expect(view([], false, false)).not.toMatch(
      /<button[^>]*disabled=""[^>]*>(?:(?!<button).)*New chat</
    );
  });

  it("disables starting and deleting while a reply streams", () => {
    const html = view([chat("a", "Chapter one")], true);
    expect(html).toMatch(/disabled=""[^>]*aria-label="Delete Thesis"/);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>.*New chat</);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>Chapter one</);
  });
});
