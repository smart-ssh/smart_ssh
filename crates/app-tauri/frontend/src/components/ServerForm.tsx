import { type FormEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  clearServerSudoPassword,
  commandErrorCode,
  commandErrorMessage,
  createServer,
  deleteServer,
  getServer,
  previewEffectiveNotes,
  testConnection,
  trustHostKey,
  updateLocalServerNotes,
  updateLocalServerTags,
  updateServer,
} from "../api";
import { translateErrorCode } from "../errorCodes";
import { pickAndReadTextFile } from "../fileDialog";
import { loadRiskClassifierSettings } from "../riskSettings";
import type {
  AuthMethodInput,
  AuthMethodKind,
  GroupDto,
  HostKeyInfo,
  PostIngestPolicy,
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

/** `t` wird als Parameter durchgereicht statt selbst `useTranslation()`
 * aufzurufen — diese Funktion läuft außerhalb einer Komponente, Hooks sind
 * hier nicht erlaubt. */
function authKindLabels(t: (key: string) => string): Record<AuthMethodKind | "privateKey", string> {
  return {
    password: t("serverForm.authKind.password"),
    private_key: t("serverForm.authKind.privateKey"),
    privateKey: t("serverForm.authKind.privateKey"),
    agent: t("serverForm.authKind.agent"),
    certificate: t("serverForm.authKind.certificate"),
  };
}

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
  const { t } = useTranslation();
  const AUTH_KIND_LABELS = authKindLabels(t);
  const isCreate = serverId === null;

  const [loaded, setLoaded] = useState<ServerDto | null>(null);
  // Spec 0032, Abschnitt 3: nur bekannt, sobald `loaded` geladen ist (der
  // lokale Pseudo-Server ist nie `serverId === null`, also nie `isCreate`).
  const isLocal = loaded?.isLocal ?? false;
  const [localNotes, setLocalNotes] = useState("");
  const [savingLocalNotes, setSavingLocalNotes] = useState(false);
  const [savingLocalTags, setSavingLocalTags] = useState(false);
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("22");
  const [username, setUsername] = useState("");
  const [groupId, setGroupId] = useState<string | null>(defaultGroupId);
  const [jumpHost, setJumpHost] = useState<string | null>(null);
  const [tags, setTags] = useState<string[]>([]);
  const [tagDraft, setTagDraft] = useState("");
  const [auth, setAuth] = useState<AuthFormState>({ kind: "password", value: "" });
  // Spec 0018, Abschnitt 4: leer = unverändert (bei Update), separater
  // "Entfernen"-Weg für einen bereits gesetzten Wert (s. `handleClearSudoPassword`).
  const [sudoPassword, setSudoPassword] = useState("");
  const [hasSudoPassword, setHasSudoPassword] = useState(false);
  const [clearingSudoPassword, setClearingSudoPassword] = useState(false);
  const [postIngestPolicy, setPostIngestPolicy] = useState<PostIngestPolicy>("balanced");
  const [aiInjectionCheckEnabled, setAiInjectionCheckEnabled] = useState(false);
  // Spec 0039, Abschnitt 5.2: die Checkbox ist nur bedienbar, wenn ein
  // Zweitmeinungs-Provider konfiguriert ist (dieselbe Voraussetzung wie
  // beim Backend-`Session::injection_check_provider`, s. dortiger
  // Kommentar) — sonst bliebe die Einstellung wirkungslos, ohne dass das
  // sichtbar wäre.
  const [secondOpinionAvailable, setSecondOpinionAvailable] = useState(false);

  useEffect(() => {
    loadRiskClassifierSettings()
      .then((settings) => setSecondOpinionAvailable(settings.enabled && settings.providerId !== null))
      .catch(() => setSecondOpinionAvailable(false));
  }, []);

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
      setSudoPassword("");
      setHasSudoPassword(false);
      setPostIngestPolicy("balanced");
      setAiInjectionCheckEnabled(false);
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
        setSudoPassword("");
        setHasSudoPassword(server.hasSudoPassword);
        setLocalNotes(server.notes);
        setPostIngestPolicy(server.postIngestPolicy);
        setAiInjectionCheckEnabled(server.aiInjectionCheckEnabled);
      })
      .catch((err) => setError(commandErrorMessage(err)));
  }, [serverId, defaultGroupId]);

  // Spec 0032, Abschnitt 6: der lokale Pseudo-Server kann nicht als
  // Jump-Host referenziert werden.
  const possibleJumpHosts = useMemo(
    () => allServers.filter((s) => s.id !== serverId && !s.isLocal),
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
    sudoPassword: sudoPassword === "" ? null : sudoPassword,
    postIngestPolicy,
    aiInjectionCheckEnabled: secondOpinionAvailable && aiInjectionCheckEnabled,
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
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    } finally {
      setSaving(false);
    }
  };

  const handleClearSudoPassword = async () => {
    if (!serverId) return;
    setClearingSudoPassword(true);
    setError(null);
    try {
      await clearServerSudoPassword(serverId);
      setHasSudoPassword(false);
    } catch (err) {
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    } finally {
      setClearingSudoPassword(false);
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
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
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
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
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
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    }
  };

  const handleAddTag = () => {
    const value = tagDraft.trim();
    if (value && !tags.includes(value)) setTags([...tags, value]);
    setTagDraft("");
  };

  // Spec 0032, Abschnitt 3: der lokale Pseudo-Server hat keinen normalen
  // Formular-Submit (`create_server`/`update_server` lehnen seine ID ab,
  // s. `crate::commands::update_server`) — Tags/Notizen werden stattdessen
  // über eigene, dedizierte Befehle gespeichert.
  const handleSaveLocalTags = async () => {
    setSavingLocalTags(true);
    setError(null);
    try {
      await updateLocalServerTags(tags);
      onSaved();
    } catch (err) {
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    } finally {
      setSavingLocalTags(false);
    }
  };

  const handleSaveLocalNotes = async () => {
    setSavingLocalNotes(true);
    setError(null);
    try {
      await updateLocalServerNotes(localNotes);
      onSaved();
    } catch (err) {
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    } finally {
      setSavingLocalNotes(false);
    }
  };

  const handlePreview = async () => {
    if (!serverId) return;
    setPreviewLoading(true);
    setError(null);
    try {
      setPreview(await previewEffectiveNotes(serverId));
    } catch (err) {
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    } finally {
      setPreviewLoading(false);
    }
  };

  // Spec 0032, Abschnitt 3/5: host/port/username/auth/jump-host sind für
  // den lokalen Pseudo-Server bedeutungslos (kein `servers`-Datensatz, kein
  // Verbindungsaufbau) und deshalb komplett ausgeblendet statt nur
  // deaktiviert — nur Name (fest "Localhost"), Notizen und Tags bleiben
  // sichtbar/editierbar. Kein Löschen-Button (existiert nicht als
  // löschbare Zeile) und kein Verbindungstest (es gibt keine Verbindung zu
  // testen).
  if (loaded && isLocal) {
    return (
      <div className="max-w-2xl space-y-6 p-4">
        <h2 className="font-heading text-lg font-semibold tracking-wide text-slate-100">{loaded.name}</h2>
        <p className="text-sm text-slate-400">{t("serverForm.localHint")}</p>

        {error && <p className="text-sm text-red-400">{error}</p>}

        <div className="block text-sm text-slate-300">
          {t("serverForm.tags")}
          <div className="mt-1 flex flex-wrap items-center gap-1 rounded border border-slate-600 bg-slate-900 p-1.5">
            {tags.map((tag) => (
              <span
                key={tag}
                className="flex items-center gap-1 rounded bg-slate-700 px-2 py-0.5 text-xs text-slate-100"
              >
                {tag}
                <button
                  type="button"
                  onClick={() => setTags(tags.filter((tagValue) => tagValue !== tag))}
                  className="text-slate-400 hover:text-white"
                  aria-label={t("serverForm.removeTagAria", { tag })}
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
              placeholder={t("serverForm.tagPlaceholder")}
              className="flex-1 bg-transparent px-1 py-0.5 text-sm text-slate-100 outline-none"
            />
          </div>
          <button
            type="button"
            onClick={handleSaveLocalTags}
            disabled={savingLocalTags}
            className="mt-2 rounded bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
          >
            {savingLocalTags ? t("common.saving") : t("common.save")}
          </button>
        </div>

        <div className="block text-sm text-slate-300">
          {t("common.notes")}
          <textarea
            value={localNotes}
            onChange={(e) => setLocalNotes(e.target.value)}
            rows={6}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-sm text-slate-100"
          />
          <button
            type="button"
            onClick={handleSaveLocalNotes}
            disabled={savingLocalNotes}
            className="mt-2 rounded bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
          >
            {savingLocalNotes ? t("common.saving") : t("common.save")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="max-w-2xl space-y-6 p-4">
      <h2 className="font-heading text-lg font-semibold tracking-wide text-slate-100">
        {isCreate ? t("serverForm.titleNew") : t("serverForm.titleExisting", { name: loaded?.name ?? "" })}
      </h2>

      <form onSubmit={handleSubmit} className="space-y-3">
        <div className="grid grid-cols-2 gap-3">
          <label className="block text-sm text-slate-300">
            {t("common.name")}
            <input
              type="text"
              required
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>
          <label className="block text-sm text-slate-300">
            {t("serverForm.host")}
            <input
              type="text"
              required
              value={host}
              onChange={(e) => setHost(e.target.value)}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>
          <label className="block text-sm text-slate-300">
            {t("serverForm.port")}
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
            {t("serverForm.username")}
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
          {t("serverForm.group")}
          <select
            value={groupId ?? ""}
            onChange={(e) => setGroupId(e.target.value || null)}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          >
            <option value="">{t("serverForm.noGroup")}</option>
            {allGroups.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name}
              </option>
            ))}
          </select>
        </label>

        <label className="block text-sm text-slate-300">
          {t("serverForm.jumpHost")}
          <select
            value={jumpHost ?? ""}
            onChange={(e) => setJumpHost(e.target.value || null)}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          >
            <option value="">{t("serverForm.noJumpHost")}</option>
            {possibleJumpHosts.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        </label>

        <div className="block text-sm text-slate-300">
          {t("serverForm.tags")}
          <div className="mt-1 flex flex-wrap items-center gap-1 rounded border border-slate-600 bg-slate-900 p-1.5">
            {tags.map((tag) => (
              <span
                key={tag}
                className="flex items-center gap-1 rounded bg-slate-700 px-2 py-0.5 text-xs text-slate-100"
              >
                {tag}
                <button
                  type="button"
                  onClick={() => setTags(tags.filter((tagValue) => tagValue !== tag))}
                  className="text-slate-400 hover:text-white"
                  aria-label={t("serverForm.removeTagAria", { tag })}
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
              placeholder={t("serverForm.tagPlaceholder")}
              className="flex-1 bg-transparent px-1 py-0.5 text-sm text-slate-100 outline-none"
            />
          </div>
        </div>

        <fieldset className="rounded border border-slate-700 p-3">
          <legend className="px-1 text-sm text-slate-300">{t("serverForm.authFieldset")}</legend>
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
              {t("serverForm.password")}{" "}
              {!isCreate && <span className="text-slate-500">{t("serverForm.unchangedHint")}</span>}
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
                    {t("serverForm.privateKeyLabel")}{" "}
                    {!isCreate && <span className="text-slate-500">{t("serverForm.unchangedHint")}</span>}
                  </span>
                  <button
                    type="button"
                    onClick={async () => {
                      const content = await pickAndReadTextFile(
                        t("serverForm.choosePrivateKeyDialogTitle"),
                      );
                      if (content !== null) setAuth({ ...auth, keyContent: content });
                    }}
                    className="rounded bg-slate-700 px-2 py-0.5 text-xs hover:bg-slate-600"
                  >
                    {t("serverForm.chooseFile")}
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
                {t("serverForm.passphraseOptional")}
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
            <p className="text-sm text-slate-400">{t("serverForm.agentHint")}</p>
          )}

          {auth.kind === "certificate" && (
            <div className="space-y-2">
              <div>
                <div className="flex items-center justify-between">
                  <span className="text-sm text-slate-300">
                    {t("serverForm.certificateLabel")}{" "}
                    {!isCreate && <span className="text-slate-500">{t("serverForm.unchangedHint")}</span>}
                  </span>
                  <button
                    type="button"
                    onClick={async () => {
                      const content = await pickAndReadTextFile(
                        t("serverForm.chooseCertificateDialogTitle"),
                      );
                      if (content !== null) setAuth({ ...auth, certContent: content });
                    }}
                    className="rounded bg-slate-700 px-2 py-0.5 text-xs hover:bg-slate-600"
                  >
                    {t("serverForm.chooseFile")}
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
                    {t("serverForm.associatedKey")}{" "}
                    {!isCreate && <span className="text-slate-500">{t("serverForm.unchangedHint")}</span>}
                  </span>
                  <button
                    type="button"
                    onClick={async () => {
                      const content = await pickAndReadTextFile(t("serverForm.chooseKeyDialogTitle"));
                      if (content !== null) setAuth({ ...auth, keyContent: content });
                    }}
                    className="rounded bg-slate-700 px-2 py-0.5 text-xs hover:bg-slate-600"
                  >
                    {t("serverForm.chooseFile")}
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

        <fieldset className="rounded border border-slate-700 p-3">
          <legend className="px-1 text-sm text-slate-300">{t("serverForm.sudoFieldset")}</legend>
          <p className="mb-2 text-xs text-slate-500">{t("serverForm.sudoHint")}</p>
          <label className="block text-sm text-slate-300">
            {t("serverForm.sudoLabel")}{" "}
            <span className="text-slate-500">
              {hasSudoPassword ? t("serverForm.sudoUnchangedStored") : t("serverForm.sudoUnchanged")}
            </span>
            <input
              type="password"
              value={sudoPassword}
              onChange={(e) => setSudoPassword(e.target.value)}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>
          {hasSudoPassword && (
            <button
              type="button"
              onClick={handleClearSudoPassword}
              disabled={clearingSudoPassword}
              className="mt-2 rounded bg-slate-800 px-2 py-1 text-xs text-red-300 hover:bg-slate-700 disabled:opacity-50"
            >
              {clearingSudoPassword ? t("common.removing") : t("serverForm.removeSudoPassword")}
            </button>
          )}
        </fieldset>

        <fieldset className="rounded border border-slate-700 p-3">
          <legend className="px-1 text-sm text-slate-300">
            {t("serverForm.postIngestFieldset")}
          </legend>
          <p className="mb-2 text-xs text-slate-500">{t("serverForm.postIngestHint")}</p>
          <div className="space-y-2">
            {(["strict", "balanced", "standard"] as PostIngestPolicy[]).map((option) => (
              <label key={option} className="flex items-start gap-2 text-sm text-slate-300">
                <input
                  type="radio"
                  name="postIngestPolicy"
                  value={option}
                  checked={postIngestPolicy === option}
                  onChange={() => setPostIngestPolicy(option)}
                  className="mt-1"
                />
                <span>
                  <span className="font-medium">{t(`serverForm.postIngest.${option}.label`)}</span>
                  <br />
                  <span className="text-xs text-slate-500">
                    {t(`serverForm.postIngest.${option}.description`)}
                  </span>
                </span>
              </label>
            ))}
          </div>
        </fieldset>

        <fieldset className="rounded border border-slate-700 p-3">
          <legend className="px-1 text-sm text-slate-300">
            {t("serverForm.aiInjectionCheckFieldset")}
          </legend>
          <label className="flex items-start gap-2 text-sm text-slate-300">
            <input
              type="checkbox"
              checked={aiInjectionCheckEnabled}
              disabled={!secondOpinionAvailable}
              onChange={(e) => setAiInjectionCheckEnabled(e.target.checked)}
              className="mt-1"
            />
            <span>{t("serverForm.aiInjectionCheckLabel")}</span>
          </label>
          <p className="mt-2 text-xs text-slate-500">
            {secondOpinionAvailable
              ? t("serverForm.aiInjectionCheckHint")
              : t("serverForm.aiInjectionCheckUnavailableHint")}
          </p>
        </fieldset>

        {error && <p className="text-sm text-red-400">{error}</p>}

        <div className="flex flex-wrap items-center gap-2">
          <button
            type="submit"
            disabled={saving}
            className="rounded bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
          >
            {saving ? t("common.saving") : isCreate ? t("common.create") : t("common.save")}
          </button>
          <button
            type="button"
            onClick={runTest}
            disabled={testing}
            className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700 disabled:opacity-50"
          >
            {testing ? t("common.testing") : t("serverForm.testConnection")}
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
              {previewLoading ? t("common.loading") : t("serverForm.contextPreview")}
            </button>
            {preview !== null && (
              <pre className="mt-2 max-h-64 overflow-y-auto whitespace-pre-wrap rounded border border-slate-700 bg-slate-950 p-3 text-xs text-slate-300">
                {preview || t("serverForm.noContext")}
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
              {deleting ? t("common.deleting") : t("serverForm.deleteServer")}
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
  const { t } = useTranslation();
  switch (result.kind) {
    case "success":
      return <span className="text-sm text-emerald-400">{t("serverForm.testResult.success")}</span>;
    case "authenticationFailed":
      return <span className="text-sm text-red-400">{t("serverForm.testResult.authFailed")}</span>;
    case "hostKeyUnknown":
      return (
        <span className="text-sm text-amber-400">{t("serverForm.testResult.hostKeyUnknown")}</span>
      );
    case "hostKeyMismatch":
      return <span className="text-sm text-red-400">{t("serverForm.testResult.hostKeyMismatch")}</span>;
    case "networkError":
      return (
        <span className="text-sm text-red-400">
          {t("serverForm.testResult.networkError", { message: result.message })}
        </span>
      );
    case "timeout":
      return <span className="text-sm text-red-400">{t("serverForm.testResult.timeout")}</span>;
  }
}
