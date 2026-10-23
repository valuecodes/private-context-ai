import { describe, expect, it, vi } from "vitest";

import type { Project } from "~/lib/types";

import type { ProjectsState } from "./use-projects";
import { initialProjects, projectsReducer } from "./use-projects";

vi.mock("~/lib/ipc");

const project = (id: string, name: string, createdAt = "x"): Project => ({
  id,
  name,
  instructions: "",
  createdAt,
  updatedAt: createdAt,
  autoMemory: true,
});

const names = (state: ProjectsState): string[] =>
  state.projects.map(({ name }) => name);

describe("projectsReducer", () => {
  it("starts loading", () => {
    expect(initialProjects.phase).toBe("loading");
    expect(initialProjects.projects).toEqual([]);
  });

  it("sorts the loaded projects by name", () => {
    const state = projectsReducer(initialProjects, {
      type: "loaded",
      projects: [
        project("a", "Taxes"),
        project("b", "äiti"),
        project("c", "Art"),
      ],
    });
    expect(state.phase).toBe("ready");
    expect(names(state)).toEqual(["äiti", "Art", "Taxes"]);
  });

  it("keeps a project created while the list was loading", () => {
    let state = projectsReducer(initialProjects, {
      type: "saved",
      project: project("new", "New"),
    });
    state = projectsReducer(state, {
      type: "loaded",
      projects: [project("old", "Old")],
    });
    expect(names(state)).toEqual(["New", "Old"]);
  });

  it("replaces a renamed or updated project and resorts", () => {
    let state = projectsReducer(initialProjects, {
      type: "loaded",
      projects: [project("a", "Alpha"), project("b", "Beta")],
    });
    state = projectsReducer(state, {
      type: "saved",
      project: { ...project("a", "Zeta"), instructions: "Cite sources." },
    });
    expect(names(state)).toEqual(["Beta", "Zeta"]);
    expect(state.projects[1]?.instructions).toBe("Cite sources.");
  });

  it("removes a deleted project", () => {
    const state = projectsReducer(
      projectsReducer(initialProjects, {
        type: "loaded",
        projects: [project("a", "Alpha"), project("b", "Beta")],
      }),
      { type: "deleted", id: "a" }
    );
    expect(names(state)).toEqual(["Beta"]);
  });

  it("keeps the list on a failure and clears the error on success", () => {
    const error = { kind: "invalidInput", message: "bad name" } as const;
    let state = projectsReducer(
      projectsReducer(initialProjects, {
        type: "loaded",
        projects: [project("a", "Alpha")],
      }),
      { type: "failed", error }
    );
    expect(state.error).toEqual(error);
    expect(names(state)).toEqual(["Alpha"]);
    state = projectsReducer(state, {
      type: "saved",
      project: project("b", "Beta"),
    });
    expect(state.error).toBeNull();
  });
});
