import { type FormEvent, useEffect, useState } from "react";
import {
  addAiProvider,
  commandErrorMessage,
  deleteAiProvider,
  listAiProviders,
  setActiveAiProvider,
} from "../api";
import {
  type AiProviderConfigDto,
  type AiProviderConfigInput,
  type ProviderType,
  PROVIDER_TYPE_LABELS,
  needsBaseUrl,
} from "../types";

const PROVIDER_TYPES: ProviderType[] = [
  "openai",
  "anthropic",
  "generic_openai_compatible",
  "ollama",
];

function emptyForm(): AiProviderConfigInput {
  return {
    providerType: "openai",
    displayName: "",
    baseUrl: null,
    model: "",
    supportsNativeToolCalling: true,
    apiKey: "",
  };
}

interface AiProviderSettingsProps {
  onClose: () => void;
  /** Löst neu laden von `list_ai_providers` im Elternscreen aus (z. B. für
   * den "kein Provider konfiguriert"-Hinweis), sobald sich hier etwas
   * ändert — kein globaler State-Store in Teil 1, dafür reicht ein
   * simpler Callback. */
  onProvidersChanged: () => void;
}

export function AiProviderSettings({ onClose, onProvidersChanged }: AiProviderSettingsProps) {
  const [providers, setProviders] = useState<AiProviderConfigDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<AiProviderConfigInput>(emptyForm());
  const [submitting, setSubmitting] = useState(false);

  const reload = () => {
    listAiProviders()
      .then(setProviders)
      .catch((err) => setError(commandErrorMessage(err)));
  };

  useEffect(reload, []);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      await addAiProvider(form);
      setForm(emptyForm());
      reload();
      onProvidersChanged();
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setSubmitting(false);
    }
  };

  const handleDelete = async (id: string) => {
    setError(null);
    try {
      await deleteAiProvider(id);
      reload();
      onProvidersChanged();
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

  const handleSetActive = async (id: string) => {
    setError(null);
    try {
      await setActiveAiProvider(id);
      reload();
      onProvidersChanged();
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

  return (
    <div className="fixed inset-0 flex items-center justify-center bg-black/50 p-4">
      <div className="max-h-[90vh] w-full max-w-lg overflow-y-auto rounded-lg bg-slate-800 p-6 shadow-xl">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold text-slate-100">AI-Provider</h2>
          <button
            type="button"
            onClick={onClose}
            className="text-slate-400 hover:text-slate-100"
            aria-label="Schließen"
          >
            ✕
          </button>
        </div>

        {error && (
          <p className="mb-4 rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>
        )}

        <ul className="mb-6 divide-y divide-slate-700 rounded-md border border-slate-700">
          {providers.length === 0 && (
            <li className="px-4 py-3 text-sm text-slate-400">Noch kein Provider konfiguriert.</li>
          )}
          {providers.map((provider) => (
            <li key={provider.id} className="flex items-center justify-between gap-3 px-4 py-3">
              <div>
                <p className="font-medium text-slate-100">
                  {provider.displayName}
                  {provider.isActive && (
                    <span className="ml-2 rounded bg-emerald-900 px-2 py-0.5 text-xs text-emerald-300">
                      aktiv
                    </span>
                  )}
                </p>
                <p className="text-sm text-slate-400">
                  {PROVIDER_TYPE_LABELS[provider.providerType]} · {provider.model}
                </p>
              </div>
              <div className="flex shrink-0 gap-2">
                {!provider.isActive && (
                  <button
                    type="button"
                    onClick={() => handleSetActive(provider.id)}
                    className="rounded bg-slate-700 px-2 py-1 text-xs text-slate-100 hover:bg-slate-600"
                  >
                    Aktiv setzen
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => handleDelete(provider.id)}
                  className="rounded bg-red-900 px-2 py-1 text-xs text-red-200 hover:bg-red-800"
                >
                  Löschen
                </button>
              </div>
            </li>
          ))}
        </ul>

        <form onSubmit={handleSubmit} className="space-y-3">
          <h3 className="text-sm font-semibold text-slate-200">Provider hinzufügen</h3>

          <label className="block text-sm text-slate-300">
            Typ
            <select
              value={form.providerType}
              onChange={(e) =>
                setForm({ ...form, providerType: e.target.value as ProviderType })
              }
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            >
              {PROVIDER_TYPES.map((type) => (
                <option key={type} value={type}>
                  {PROVIDER_TYPE_LABELS[type]}
                </option>
              ))}
            </select>
          </label>

          <label className="block text-sm text-slate-300">
            Name
            <input
              type="text"
              required
              value={form.displayName}
              onChange={(e) => setForm({ ...form, displayName: e.target.value })}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>

          <label className="block text-sm text-slate-300">
            Modell
            <input
              type="text"
              required
              placeholder="z. B. claude-sonnet-5, gpt-5, llama3"
              value={form.model}
              onChange={(e) => setForm({ ...form, model: e.target.value })}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>

          {needsBaseUrl(form.providerType) && (
            <label className="block text-sm text-slate-300">
              Base-URL
              <input
                type="text"
                required
                placeholder="https://…"
                value={form.baseUrl ?? ""}
                onChange={(e) => setForm({ ...form, baseUrl: e.target.value })}
                className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
              />
            </label>
          )}

          <label className="block text-sm text-slate-300">
            API-Key
            <input
              type="password"
              required
              value={form.apiKey}
              onChange={(e) => setForm({ ...form, apiKey: e.target.value })}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>

          <label className="flex items-center gap-2 text-sm text-slate-300">
            <input
              type="checkbox"
              checked={form.supportsNativeToolCalling}
              onChange={(e) =>
                setForm({ ...form, supportsNativeToolCalling: e.target.checked })
              }
            />
            Natives Tool-Calling unterstützt
          </label>

          <button
            type="submit"
            disabled={submitting}
            className="w-full rounded bg-indigo-600 px-3 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
          >
            {submitting ? "Wird hinzugefügt…" : "Hinzufügen"}
          </button>
        </form>
      </div>
    </div>
  );
}
