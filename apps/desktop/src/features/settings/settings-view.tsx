import type { SubmitEvent } from "react";
import { useEffect, useState } from "react";

import { toAppError } from "~/lib/app-error";
import { appInfo, ollamaListModels, settingsGet, settingsSet } from "~/lib/ipc";
import type { LogLevel, Settings } from "~/lib/types";

const LOG_LEVELS: LogLevel[] = ["error", "warn", "info", "debug", "trace"];

const input =
  "rounded border border-zinc-300 bg-white px-2 py-1 text-sm dark:border-zinc-700 dark:bg-zinc-900";

const label = "text-sm font-medium";

type SettingsFormProps = {
  draft: Settings;
  /** Installed model names. */
  models: string[];
  logDir: string;
  saving: boolean;
  /** Rust's loopback check is the only one; its message is shown here. */
  urlError: string | null;
  formError: string | null;
  saved: boolean;
  onChange: (draft: Settings) => void;
  onSubmit: () => void;
};

const SettingsForm = ({
  draft,
  models,
  logDir,
  saving,
  urlError,
  formError,
  saved,
  onChange,
  onSubmit,
}: SettingsFormProps) => {
  // A saved default that is no longer installed stays selectable.
  const modelOptions =
    draft.defaultModel === "" || models.includes(draft.defaultModel)
      ? models
      : [draft.defaultModel, ...models];

  const submit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    onSubmit();
  };

  return (
    <form onSubmit={submit} className="flex max-w-lg flex-col gap-4">
      <div className="flex flex-col gap-1">
        <label htmlFor="base-url" className={label}>
          Ollama URL
        </label>
        <input
          id="base-url"
          value={draft.ollamaBaseUrl}
          onChange={(event) => {
            onChange({ ...draft, ollamaBaseUrl: event.target.value });
          }}
          aria-invalid={urlError === null ? undefined : true}
          aria-describedby={
            urlError === null ? "base-url-hint" : "base-url-error"
          }
          spellCheck={false}
          className={input}
        />
        {urlError === null ? (
          <p
            id="base-url-hint"
            className="text-xs text-zinc-500 dark:text-zinc-400"
          >
            Plain http on this computer only (127.0.0.1, ::1 or localhost).
          </p>
        ) : (
          <p
            id="base-url-error"
            className="text-xs text-red-600 dark:text-red-400"
          >
            {urlError}
          </p>
        )}
      </div>

      <div className="flex flex-col gap-1">
        <label htmlFor="default-model" className={label}>
          Default model
        </label>
        <select
          id="default-model"
          value={draft.defaultModel}
          onChange={(event) => {
            onChange({ ...draft, defaultModel: event.target.value });
          }}
          className={input}
        >
          <option value="">None (first installed model)</option>
          {modelOptions.map((name) => (
            <option key={name} value={name}>
              {models.includes(name) ? name : `${name} (not installed)`}
            </option>
          ))}
        </select>
      </div>

      <div className="flex flex-col gap-1">
        <label htmlFor="log-level" className={label}>
          Log level
        </label>
        <select
          id="log-level"
          value={draft.logLevel}
          onChange={(event) => {
            const level = LOG_LEVELS.find(
              (value) => value === event.target.value
            );
            if (level !== undefined) {
              onChange({ ...draft, logLevel: level });
            }
          }}
          className={input}
        >
          {LOG_LEVELS.map((level) => (
            <option key={level} value={level}>
              {level}
            </option>
          ))}
        </select>
      </div>

      <div className="flex flex-col gap-1">
        <span className={label}>Log folder</span>
        <code className="text-xs break-all text-zinc-600 dark:text-zinc-400">
          {logDir === "" ? "—" : logDir}
        </code>
      </div>

      {formError !== null && (
        <p role="alert" className="text-sm text-red-600 dark:text-red-400">
          {formError}
        </p>
      )}
      <div className="flex items-center gap-2">
        <button
          type="submit"
          disabled={saving}
          className="rounded bg-zinc-900 px-3 py-1 text-sm font-medium text-white hover:bg-zinc-700 disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-300"
        >
          Save
        </button>
        {saved && (
          <span
            role="status"
            className="text-xs text-zinc-500 dark:text-zinc-400"
          >
            Saved
          </span>
        )}
      </div>
    </form>
  );
};

type SettingsViewProps = {
  /** Called with what was stored, after a successful save. */
  onSaved: (settings: Settings) => void;
};

type Errors = { url: string | null; form: string | null };

const NO_ERRORS: Errors = { url: null, form: null };

/** Loads the settings, installed models and log folder, and saves edits. */
const SettingsView = ({ onSaved }: SettingsViewProps) => {
  const [draft, setDraft] = useState<Settings | null>(null);
  const [models, setModels] = useState<string[]>([]);
  const [logDir, setLogDir] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [errors, setErrors] = useState<Errors>(NO_ERRORS);
  // Bumped after each save, so a new base URL lists its own models.
  const [modelsAttempt, setModelsAttempt] = useState(0);

  // Optional: Ollama may be down. A failure clears the list, so models of a
  // previous URL are never offered.
  useEffect(() => {
    let cancelled = false;
    void ollamaListModels().then(
      (list) => {
        if (!cancelled) {
          setModels(list.map((model) => model.name));
        }
      },
      () => {
        if (!cancelled) {
          setModels([]);
        }
      }
    );
    return () => {
      cancelled = true;
    };
  }, [modelsAttempt]);

  useEffect(() => {
    let cancelled = false;
    void settingsGet().then(
      (settings) => {
        if (!cancelled) {
          setDraft(settings);
        }
      },
      (error: unknown) => {
        if (!cancelled) {
          setErrors({ url: null, form: toAppError(error).message });
        }
      }
    );
    // The folder is decoration.
    void appInfo().then(
      (info) => {
        if (!cancelled) {
          setLogDir(info.logDir);
        }
      },
      () => undefined
    );
    return () => {
      cancelled = true;
    };
  }, []);

  const save = async (settings: Settings) => {
    setSaving(true);
    setSaved(false);
    try {
      const stored = await settingsSet(settings);
      setDraft(stored);
      setErrors(NO_ERRORS);
      setSaved(true);
      setModelsAttempt((count) => count + 1);
      onSaved(stored);
    } catch (error: unknown) {
      const appError = toAppError(error);
      setErrors(
        appError.kind === "invalidBaseUrl"
          ? { url: appError.message, form: null }
          : { url: null, form: appError.message }
      );
    } finally {
      setSaving(false);
    }
  };

  let body = (
    <p className="text-sm text-zinc-500 dark:text-zinc-400">Loading…</p>
  );
  if (draft === null && errors.form !== null) {
    body = (
      <p role="alert" className="text-sm text-red-600 dark:text-red-400">
        {errors.form}
      </p>
    );
  } else if (draft !== null) {
    body = (
      <SettingsForm
        draft={draft}
        models={models}
        logDir={logDir}
        saving={saving}
        urlError={errors.url}
        formError={errors.form}
        saved={saved}
        onChange={(next) => {
          setDraft(next);
          setSaved(false);
          // An edited URL is no longer the one that was rejected.
          if (next.ollamaBaseUrl !== draft.ollamaBaseUrl) {
            setErrors((current) => ({ ...current, url: null }));
          }
        }}
        onSubmit={() => {
          void save(draft);
        }}
      />
    );
  }

  return (
    <section
      aria-labelledby="settings-heading"
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4"
    >
      <h2 id="settings-heading" className="text-sm font-semibold">
        Settings
      </h2>
      {body}
    </section>
  );
};

export { SettingsForm, SettingsView };
