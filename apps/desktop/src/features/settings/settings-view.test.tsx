import type { ComponentProps } from "react";
import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { AppearanceSettings, SettingsForm } from "./settings-view";

type Props = ComponentProps<typeof SettingsForm>;

const render = (overrides: Partial<Props> = {}): string =>
  renderToString(
    <SettingsForm
      draft={{
        ollamaBaseUrl: "http://127.0.0.1:11434",
        defaultModel: "",
        logLevel: "info",
      }}
      models={["llama3.2"]}
      logDir="/data/logs"
      saving={false}
      urlError={null}
      formError={null}
      saved={false}
      onChange={() => undefined}
      onSubmit={() => undefined}
      {...overrides}
    />
  );

describe("SettingsForm", () => {
  it("shows a rejected base URL inline on its field", () => {
    const html = render({
      urlError: "Ollama URL rejected: host must be a loopback address",
    });
    expect(html).toContain('aria-invalid="true"');
    expect(html).toContain('aria-describedby="base-url-error"');
    expect(html).toContain(
      "Ollama URL rejected: host must be a loopback address"
    );
    expect(html).not.toContain('role="alert"');
  });

  it("shows the loopback hint while the URL is valid", () => {
    const html = render();
    expect(html).not.toContain("aria-invalid");
    expect(html).toContain('aria-describedby="base-url-hint"');
  });

  it("lists the five log levels", () => {
    const html = render();
    for (const level of ["error", "warn", "info", "debug", "trace"]) {
      expect(html).toContain(`<option value="${level}"`);
    }
  });

  it("shows the log folder as text", () => {
    expect(render()).toContain("/data/logs");
  });

  it("keeps a saved default that is no longer installed", () => {
    const html = render({
      draft: {
        ollamaBaseUrl: "http://127.0.0.1:11434",
        defaultModel: "old-model",
        logLevel: "info",
      },
    });
    expect(html).toContain("old-model (not installed)");
    expect(html).toContain('<option value="llama3.2"');
  });

  it("announces other save errors as an alert", () => {
    const html = render({ formError: "database error: disk full" });
    expect(html).toContain('role="alert"');
    expect(html).toContain("database error: disk full");
  });
});

describe("AppearanceSettings", () => {
  it("offers System, Light and Dark with the current choice checked", () => {
    const html = renderToString(
      <AppearanceSettings theme="dark" onChange={() => undefined} />
    );
    expect(html).toMatch(/<fieldset[^>]*><legend[^>]*>Appearance<\/legend>/);
    expect(html.match(/type="radio"/g)).toHaveLength(3);
    for (const label of ["System", "Light", "Dark"]) {
      expect(html).toContain(label);
    }
    expect(html.match(/<input[^>]*checked=""[^>]*>/g)).toEqual([
      expect.stringContaining('value="dark"'),
    ]);
  });
});
