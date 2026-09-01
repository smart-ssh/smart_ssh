import { type FormEvent, useEffect, useMemo, useState } from "react";
import {
  commandErrorMessage,
  createServer,
  deleteServer,
  getServer,
  previewEffectiveNotes,
  testConnection,
  trustHostKey,
  updateServer,
} from "../api";
import { pickAndReadTextFile } from "../fileDialog";
import type {
  AuthMethodInput,
  AuthMethodKind,
  GroupDto,
  HostKeyInfo,
  ServerDto,
  ServerInput,
  TestConnectionResult,
} from "../types";
import { HostKeyDialog } from "./HostKeyDialog";
import { NotesPanel } from "./NotesPanel";

interface ServerFormProps {
  /** `null` = Neuanlage. */
  serverId: string | null;
  defaultGroupId: string | null;
  allGroups: GroupDto[];
  allServers: ServerDto[];
  onSaved: () => void;
  onDeleted: () => void;
}

const AUTH_KIND_LABELS: Record<AuthMethodKind | "privateKey", string> = {
  password: "Passwort",
  private_key: "Private Key",
  privateKey: "Private Key",
  agent: "SSH-Agent verwenden",
  certificate: "Zertifikat",
};

/** Formular-lokaler Zustand — getrennt von [`AuthMethodInput`], weil
 * Textfelder immer einen String brauchen (nie `null`), auch wenn die
 * Bedeutung "leer = unverändert" (Update) erst beim Absenden entsteht
 * (s. `toAuthMethodInput`). */
type AuthFormState =
  | { kind: "password"; value: string }
  | { kind: "privateKey"; keyContent: string; passphrase: string }
  | { kind: "agent" }
  | { kind: "certificate"; certContent: string; keyContent: string };

function authStateFromKind(kind: AuthFormState["kind"]): AuthFormState {
  switch (kind) {
    case "password":
      return { kind: "password", value: "" };
    case "privateKey":
      return { kind: "privateKey", keyContent: "", passphrase: "" };
    case "agent":
      return { kind: "agent" };
    case "certificate":
      return { kind: "certificate", certContent: "", keyContent: "" };
  }
}

/** Spec 0008, Abschnitt 4: bei `update_server` bedeutet ein leeres
 * Secret-Feld "unverändert lassen" (`null`), bei `create_server` ist es
 * schlicht noch nicht ausgefüllt (leerer String bleibt leerer String —
 * das Backend verlangt dort zwingend einen Wert und lehnt sonst ab). Die
 * Passphrase ist immer optional, auch bei Neuanlage.
 */
function toAuthMethodInput(state: AuthFormState, isCreate: boolean): AuthMethodInput {
  const orNullIfUpdate = (value: string) => (value === "" && !isCreate ? null : value);
  switch (state.kind) {
    case "password":
      return { kind: "password", value: orNullIfUpdate(state.value) };
    case "privateKey":
      return {
        kind: "privateKey",
        keyContent: orNullIfUpdate(state.keyContent),
        passphrase: state.passphrase === "" ? null : state.passphrase,
      };
    case "agent":
      return { kind: "agent" };
    case "certificate":
      return {
        kind: "certificate",
        certContent: orNullIfUpdate(state.certContent),
        keyContent: orNullIfUpdate(state.keyContent),
      };
  }
}

export function ServerForm({
  serverId,
  defaultGroupId,
  allGroups,
  allServers,
  onSaved,
  onDeleted,
}: ServerFormProps) {
  const isCreate = serverId === null;

  const [loaded, setLoaded] = useState<ServerDto | null>(null);
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("22");
  const [username, setUsername] = useState("");
  const [groupId, setGroupId] = useState<string | null>(defaultGroupId);
  const [jumpHost, setJumpHost] = useState<string | null>(null);
  const [tags, setTags] = useState<string[]>([]);
  const [tagDraft, setTagDraft] = useState("");
  const [auth, setAuth] = useState<AuthFormState>({ kind: "password", value: "" });

  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<TestConnectionResult | null>(null);
  const [pendingHostKey, setPendingHostKey] = useState<HostKeyInfo | null>(null);

  const [preview, setPreview] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);

  useEffect(() => {
    setError(null);
    setTestResult(null);
    setPendingHostKey(null);
    setPreview(null);
    if (serverId === null) {
      setLoaded(null);
      setName("");
      setHost("");
      setPort("22");
      setUsername("");
      setGroupId(defaultGroupId);
      setJumpHost(null);
      setTags([]);
      setAuth({ kind: "password", value: "" });
      return;
    }
    getServer(serverId)
      .then((server) => {
        setLoaded(server);
        setName(server.name);
        setHost(server.host);
        setPort(String(server.port));
        setUsername(server.username);
        setGroupId(server.groupId);
        setJumpHost(server.jumpHost);
        setTags(server.tags);
        setAuth(authStateFromKind(server.authKind === "private_key" ? "privateKey" : server.authKind));
      })
      .catch((err) => setError(commandErrorMessage(err)));
  }, [serverId, defaultGroupId]);

  const possibleJumpHosts = useMemo(
    () => allServers.filter((s) => s.id !== serverId),
    [allServers, serverId],
  );

  const buildInput = (): ServerInput => ({
    name,
    host,
    port: Number(port),
    username,
    groupId,
    tags,
    auth: toAuthMethodInput(auth, isCreate),
    jumpHost,
  });

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setSaving(true);
    setError(null);
    try {
      if (isCreate) {
        await createServer(buildInput());
      } else if (serverId) {
        await updateServer(serverId, buildInput());
      }
      onSaved();
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!serverId) return;
    setDeleting(true);
    setError(null);
    try {
      await deleteServer(serverId);
      onDeleted();
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setDeleting(false);
    }
  };

  const runTest = async () => {
    setTesting(true);
    setError(null);
    setTestResult(null);
    try {
      const result = await testConnection(buildInput(), serverId ?? undefined);
      setTestResult(result);
      if (result.kind === "hostKeyUnknown" || result.kind === "hostKeyMismatch") {
        setPendingHostKey({
          host: result.host,
          port: result.port,
          kind: result.kind === "hostKeyUnknown" ? "unknown" : "mismatch",
          fingerprint: result.kind === "hostKeyUnknown" ? result.fingerprint : result.actualFingerprint,
          expectedFingerprint: result.kind === "hostKeyMismatch" ? result.expectedFingerprint : null,
        });
      }
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setTesting(false);
    }
  };

  const handleTrustAndRetest = async () => {
    if (testResult?.kind !== "hostKeyUnknown" && testResult?.kind !== "hostKeyMismatch") return;
    const { host: h, port: p, rawKey } = testResult;
    setPendingHostKey(null);
    try {
      await trustHostKey(h, p, rawKey);
      await runTest();
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

  const handleAddTag = () => {
    const value = tagDraft.trim();
    if (value && !tags.includes(value)) setTags([...tags, value]);
    setTagDraft("");
  };

  const handlePreview = async () => {
    if (!serverId) return;
    setPreviewLoading(true);
    setError(null);
    try {
      setPreview(await previewEffectiveNotes(serverId));
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setPreviewLoading(false);
    }
  };

  return (
    <div className="max-w-2xl space-y-6 p-4">
      <h2 className="font-heading text-lg font-semibold tracking-wide text-slate-100">
        {isCreate ? "Neuer Server" : `Server: ${loaded?.name ?? ""}`}
      </h2>

      <form onSubmit={handleSubmit} className="space-y-3">
        <div className="grid grid-cols-2 gap-3">
          <label className="block text-sm text-slate-300">
            Name
            <input
              type="text"
              required
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>
          <label className="block text-sm text-slate-300">
            Host
            <input
              type="text"
              required
              value={host}
              onChange={(e) => setHost(e.target.value)}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>
          <label className="block text-sm text-slate-300">
            Port
            <input
              type="number"
              required
              min={1}
              max={65535}
              value={port}
              onChange={(e) => setPort(e.target.value)}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>
          <label className="block text-sm text-slate-300">
            Benutzername
            <input
              type="text"
              required
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>
        </div>

        <label className="block text-sm text-slate-300">
          Gruppe
          <select
            value={groupId ?? ""}
            onChange={(e) => setGroupId(e.target.value || null)}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          >
            <option value="">(keine)</option>
            {allGroups.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name}
              </option>
            ))}
          </select>
        </label>

        <label className="block text-sm text-slate-300">
          Jump-Host
          <select
            value={jumpHost ?? ""}
            onChange={(e) => setJumpHost(e.target.value || null)}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          >
            <option value="">(kein Jump-Host)</option>
            {possibleJumpHosts.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        </label>

        <div className="block text-sm text-slate-300">
          Tags
          <div className="mt-1 flex flex-wrap items-center gap-1 rounded border border-slate-600 bg-slate-900 p-1.5">
            {tags.map((tag) => (
              <span
                key={tag}
                className="flex items-center gap-1 rounded bg-slate-700 px-2 py-0.5 text-xs text-slate-100"
              >
                {tag}
                <button
                  type="button"
                  onClick={() => setTags(tags.filter((t) => t !== tag))}
                  className="text-slate-400 hover:text-white"
                  aria-label={`Tag ${tag} entfernen`}
                >
                  ✕
                </button>
              </span>
            ))}
            <input
              type="text"
              value={tagDraft}
              onChange={(e) => setTagDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === ",") {
                  e.preventDefault();
                  handleAddTag();
                }
              }}
              onBlur={handleAddTag}
              placeholder="Tag + Enter"
              className="flex-1 bg-transparent px-1 py-0.5 text-sm text-slate-100 outline-none"
            />
          </div>
        </div>

        <fieldset className="rounded border border-slate-700 p-3">
          <legend className="px-1 text-sm text-slate-300">Authentifizierung</legend>
          <select
            value={auth.kind}
            onChange={(e) => setAuth(authStateFromKind(e.target.value as AuthFormState["kind"]))}
            className="mb-3 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          >
            <option value="password">{AUTH_KIND_LABELS.password}</option>
            <option value="privateKey">{AUTH_KIND_LABELS.privateKey}</option>
            <option value="agent">{AUTH_KIND_LABELS.agent}</option>
            <option value="certificate">{AUTH_KIND_LABELS.certificate}</option>
          </select>

          {auth.kind === "password" && (
            <label className="block text-sm text-slate-300">
              Passwort {!isCreate && <span className="text-slate-500">(leer = unverändert)</span>}
              <input
                type="password"
                required={isCreate}
                value={auth.value}
                onChange={(e) => setAuth({ kind: "password", value: e.target.value })}
                className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
              />
            </label>
          )}

          {auth.kind === "privateKey" && (
            <div className="space-y-2">
              <div>
                <div className="flex items-center justify-between">
                  <span className="text-sm text-slate-300">
                    Private Key {!isCreate && <span className="text-slate-500">(leer = unverändert)</span>}
                  </span>
                  <button
                    type="button"
                    onClick={async () => {
                      const content = await pickAndReadTextFile("Private Key auswählen");
                      if (content !== null) setAuth({ ...auth, keyContent: content });
                    }}
                    className="rounded bg-slate-700 px-2 py-0.5 text-xs hover:bg-slate-600"
                  >
                    Datei wählen…
                  </button>
                </div>
                <textarea
                  required={isCreate}
                  value={auth.keyContent}
                  onChange={(e) => setAuth({ ...auth, keyContent: e.target.value })}
                  rows={4}
                  className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 font-mono text-xs text-slate-100"
                />
              </div>
              <label className="block text-sm text-slate-300">
                Passphrase (optional)
                <input
                  type="password"
                  value={auth.passphrase}
                  onChange={(e) => setAuth({ ...auth, passphrase: e.target.value })}
                  className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
                />
              </label>
            </div>
          )}

          {auth.kind === "agent" && (
            <p className="text-sm text-slate-400">
              Nutzt den lokal laufenden SSH-Agent, keine weiteren Angaben nötig.
            </p>
          )}

          {auth.kind === "certificate" && (
            <div className="space-y-2">
              <div>
                <div className="flex items-center justify-between">
                  <span className="text-sm text-slate-300">
                    Zertifikat {!isCreate && <span className="text-slate-500">(leer = unverändert)</span>}
                  </span>
                  <button
                    type="button"
                    onClick={async () => {
                      const content = await pickAndReadTextFile("Zertifikat auswählen");
                      if (content !== null) setAuth({ ...auth, certContent: content });
                    }}
                    className="rounded bg-slate-700 px-2 py-0.5 text-xs hover:bg-slate-600"
                  >
                    Datei wählen…
                  </button>
                </div>
                <textarea
                  required={isCreate}
                  value={auth.certContent}
                  onChange={(e) => setAuth({ ...auth, certContent: e.target.value })}
                  rows={3}
                  className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 font-mono text-xs text-slate-100"
                />
              </div>
              <div>
                <div className="flex items-center justify-between">
                  <span className="text-sm text-slate-300">
                    Zugehöriger Key {!isCreate && <span className="text-slate-500">(leer = unverändert)</span>}
                  </span>
                  <button
                    type="button"
                    onClick={async () => {
                      const content = await pickAndReadTextFile("Key auswählen");
                      if (content !== null) setAuth({ ...auth, keyContent: content });
                    }}
                    className="rounded bg-slate-700 px-2 py-0.5 text-xs hover:bg-slate-600"
                  >
                    Datei wählen…
                  </button>
                </div>
                <textarea
                  required={isCreate}
                  value={auth.keyContent}
                  onChange={(e) => setAuth({ ...auth, keyContent: e.target.value })}
                  rows={3}
                  className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 font-mono text-xs text-slate-100"
                />
              </div>
            </div>
          )}
        </fieldset>

        {error && <p className="text-sm text-red-400">{error}</p>}

        <div className="flex flex-wrap items-center gap-2">
          <button
            type="submit"
            disabled={saving}
            className="rounded bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
          >
            {saving ? "Speichert…" : isCreate ? "Anlegen" : "Speichern"}
          </button>
          <button
            type="button"
            onClick={runTest}
            disabled={testing}
            className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700 disabled:opacity-50"
          >
            {testing ? "Testet…" : "Verbindung testen"}
          </button>
          {testResult && <TestResultBadge result={testResult} />}
        </div>
      </form>

      {!isCreate && serverId && loaded && (
        <>
          <NotesPanel target={{ Server: serverId }} currentNotes={loaded.notes} onNotesChanged={onSaved} />

          <div>
            <button
              type="button"
              onClick={handlePreview}
              disabled={previewLoading}
              className="rounded bg-slate-800 px-3 py-1.5 text-sm hover:bg-slate-700 disabled:opacity-50"
            >
              {previewLoading ? "Lädt…" : "Kontext-Vorschau"}
            </button>
            {preview !== null && (
              <pre className="mt-2 max-h-64 overflow-y-auto whitespace-pre-wrap rounded border border-slate-700 bg-slate-950 p-3 text-xs text-slate-300">
                {preview || "(kein Kontext)"}
              </pre>
            )}
          </div>

          <div className="border-t border-slate-700 pt-4">
            <button
              type="button"
              onClick={handleDelete}
              disabled={deleting}
              className="rounded bg-red-900 px-3 py-1.5 text-sm text-red-200 hover:bg-red-800 disabled:opacity-50"
            >
              {deleting ? "Löscht…" : "Server löschen"}
            </button>
          </div>
        </>
      )}

      {pendingHostKey && (
        <HostKeyDialog
          event={pendingHostKey}
          onDecision={(decision) => {
            if (decision.decision === "trust") {
              handleTrustAndRetest();
            } else {
              setPendingHostKey(null);
            }
          }}
        />
      )}
    </div>
  );
}

function TestResultBadge({ result }: { result: TestConnectionResult }) {
  switch (result.kind) {
    case "success":
      return <span className="text-sm text-emerald-400">✓ Verbindung erfolgreich</span>;
    case "authenticationFailed":
      return <span className="text-sm text-red-400">✗ Authentifizierung fehlgeschlagen</span>;
    case "hostKeyUnknown":
      return <span className="text-sm text-amber-400">Host-Key unbekannt — Bestätigung nötig</span>;
    case "hostKeyMismatch":
      return <span className="text-sm text-red-400">⚠ Host-Key hat sich geändert</span>;
    case "networkError":
      return <span className="text-sm text-red-400">✗ Netzwerkfehler: {result.message}</span>;
    case "timeout":
      return <span className="text-sm text-red-400">✗ Zeitüberschreitung</span>;
  }
}
