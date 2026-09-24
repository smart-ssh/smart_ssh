import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  clearServerSudoPassword,
  commandErrorCode,
  commandErrorMessage,
  convertIdentityFileToKeychain,
  createServer,
  deleteServer,
  getServer,
  inspectKeyFile,
  largeNoteDialogThresholdBytes,
  previewEffectiveNotes,
  requestNoteShrink,
  testConnection,
  trustHostKey,
  updateLocalServerNotes,
  updateLocalServerTags,
  updateServer,
} from "../api";
import { translateErrorCode } from "../errorCodes";
import { onNoteShrinkSucceeded } from "../events";
import { pickAndReadTextFile, pickFilePath } from "../fileDialog";
import { loadRiskClassifierSettings } from "../riskSettings";
import type {
  AuthMethodInput,
  AuthMethodKind,
  DeleteServerResult,
  GroupDto,
  HostKeyInfo,
  KeyFileFactsDto,
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
  /** Spec 0058, Teil 2 — s. `NotesPanel.autoFocus`-Doc-Kommentar. Gilt für
   * BEIDE Notiz-Editoren dieses Formulars (den lokalen Pseudo-Server-Zweig
   * unten und den regulären `NotesPanel`-Zweig) — "Mache ich selbst" kann
   * sich auf jeden Server beziehen, den lokalen eingeschlossen. */
  autoFocusNotes?: boolean;
}

/** `t` wird als Parameter durchgereicht statt selbst `useTranslation()`
 * aufzurufen — diese Funktion läuft außerhalb einer Komponente, Hooks sind
 * hier nicht erlaubt. */
function authKindLabels(
  t: (key: string) => string,
): Record<AuthMethodKind | "privateKey" | "identityFile", string> {
  return {
    password: t("serverForm.authKind.password"),
    private_key: t("serverForm.authKind.privateKey"),
    privateKey: t("serverForm.authKind.privateKey"),
    agent: t("serverForm.authKind.agent"),
    certificate: t("serverForm.authKind.certificate"),
    identity_file: t("serverForm.authKind.identityFile"),
    identityFile: t("serverForm.authKind.identityFile"),
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
  | { kind: "certificate"; certContent: string; keyContent: string }
  | { kind: "identityFile"; path: string; passphrase: string };

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
    case "identityFile":
      return { kind: "identityFile", path: "", passphrase: "" };
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
    case "identityFile":
      // Spec 0076, A-1: der Pfad ist kein Secret und immer voll gesendet —
      // "leer = unverändert" gilt hier nicht, anders als bei `keyContent`
      // oben (ADR 0065 §5 begründet, warum der Pfad selbst nicht getrimmt
      // wird; die Leer-Prüfung übernimmt das Backend, SERVER_IDENTITY_
      // FILE_REQUIRED).
      return {
        kind: "identityFile",
        path: state.path,
        passphrase: state.passphrase === "" ? null : state.passphrase,
      };
  }
}

/** Spec 0076, B-3: zeigt den Vorab-Befund über eine Schlüsseldatei —
 * existiert sie, passen die Rechte, sieht sie wie ein OpenSSH-Schlüssel
 * aus, ist sie verschlüsselt. Ein Fehlbefund (`facts.problem`) hindert das
 * Speichern nicht, er wird nur angezeigt (B-3, letzter Satz). Eigene
 * Komponente statt Inline-JSX, damit `ServerForm` nicht noch länger wird —
 * reiner Anzeige-Baustein ohne eigenen Zustand. */
function IdentityFileFacts({
  loading,
  facts,
  emptyPath,
}: {
  loading: boolean;
  facts: KeyFileFactsDto | null;
  emptyPath: boolean;
}) {
  const { t } = useTranslation();
  if (emptyPath) return null;
  if (facts === null) {
    return loading ? (
      <p className="mt-1 text-xs text-slate-500">{t("serverForm.identityFile.checking")}</p>
    ) : null;
  }
  if (facts.problem) {
    return (
      <p className="mt-1 text-xs text-amber-400">
        {translateErrorCode(t, facts.problem.code, facts.problem.message)}
      </p>
    );
  }
  return (
    <ul className="mt-1 space-y-0.5 text-xs">
      <li className="text-emerald-400">{t("serverForm.identityFile.looksValid")}</li>
      {facts.permissionsTooOpen ? (
        <li className="text-amber-400">{t("serverForm.identityFile.permissionsTooOpen")}</li>
      ) : (
        <li className="text-slate-500">{t("serverForm.identityFile.permissionsOk")}</li>
      )}
      <li className="text-slate-500">
        {facts.encrypted
          ? t("serverForm.identityFile.encrypted")
          : t("serverForm.identityFile.notEncrypted")}
      </li>
    </ul>
  );
}

export function ServerForm({
  serverId,
  defaultGroupId,
  allGroups,
  allServers,
  onSaved,
  onDeleted,
  autoFocusNotes = false,
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
  // Spec 0058, Teil 2: derselbe Scroll+Fokus-Effekt wie `NotesPanel`s
  // `autoFocus`-Prop, hier für den lokalen Pseudo-Server (der `NotesPanel`
  // gar nicht verwendet, s. `ServerForm.tsx`s eigener Zweig unten/dortiger
  // Kommentar) separat nachgebaut. `hasAutoFocused`-Ref statt eines
  // einfachen `[]`-Effekts (der `localNotesRef.current` wäre beim
  // allerersten Render noch `null`, da `loaded` erst asynchron nachlädt) —
  // greift dadurch genau EINMAL, sobald `loaded` tatsächlich verfügbar
  // ist, nicht erneut bei jedem späteren `loaded`-Wechsel (z. B. nach dem
  // Speichern).
  const localNotesRef = useRef<HTMLTextAreaElement>(null);
  const hasAutoFocusedLocalNotes = useRef(false);
  useEffect(() => {
    if (loaded && isLocal && autoFocusNotes && !hasAutoFocusedLocalNotes.current) {
      hasAutoFocusedLocalNotes.current = true;
      localNotesRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
      // `preventScroll`: s. identischer Kommentar in `NotesPanel.tsx`.
      localNotesRef.current?.focus({ preventScroll: true });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded, isLocal]);
  // Spec 0058, Teil 1 (Etappe 5): derselbe Hinweis wie `NotesPanel`, hier
  // für den lokalen Pseudo-Server separat nachgebaut (der `NotesPanel`
  // nicht verwendet, s. Kommentar am lokalen Zweig unten).
  const [largeLocalNoteThreshold, setLargeLocalNoteThreshold] = useState<number | null>(null);
  const [localShrinkRequesting, setLocalShrinkRequesting] = useState(false);
  const [localShrinkError, setLocalShrinkError] = useState<string | null>(null);
  useEffect(() => {
    largeNoteDialogThresholdBytes()
      .then(setLargeLocalNoteThreshold)
      .catch((err) => console.error(commandErrorMessage(err)));
  }, []);
  const isLocalNoteLarge =
    largeLocalNoteThreshold !== null &&
    new TextEncoder().encode(localNotes).length >= largeLocalNoteThreshold;
  const handleSummarizeLocalNoteNow = async () => {
    if (!loaded) return;
    setLocalShrinkRequesting(true);
    setLocalShrinkError(null);
    try {
      await requestNoteShrink(loaded.id);
    } catch (err) {
      setLocalShrinkError(commandErrorMessage(err));
    } finally {
      setLocalShrinkRequesting(false);
    }
  };
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("22");
  const [username, setUsername] = useState("");
  const [groupId, setGroupId] = useState<string | null>(defaultGroupId);
  const [jumpHost, setJumpHost] = useState<string | null>(null);
  const [tags, setTags] = useState<string[]>([]);
  const [tagDraft, setTagDraft] = useState("");
  const [auth, setAuth] = useState<AuthFormState>({ kind: "password", value: "" });
  // Spec 0076, B-3: der Vorab-Befund über die im Formular gerade
  // eingetragene Schlüsseldatei — `null`, solange kein Pfad gesetzt ist
  // oder noch keine Antwort da ist. Ein Fehlbefund hindert das Speichern
  // nicht (B-3), er wird nur angezeigt.
  const [identityFacts, setIdentityFacts] = useState<KeyFileFactsDto | null>(null);
  const [identityFactsLoading, setIdentityFactsLoading] = useState(false);
  // Spec 0076, C-7: unabhängig vom obigen Entwurfszustand — bezieht sich
  // auf den tatsächlich GESPEICHERTEN Pfad (`loaded.identityFilePath`) und
  // entscheidet, ob der Überführen-Knopf wählbar ist.
  const [convertFacts, setConvertFacts] = useState<KeyFileFactsDto | null>(null);
  const [convertConfirmOpen, setConvertConfirmOpen] = useState(false);
  const [converting, setConverting] = useState(false);
  const [convertError, setConvertError] = useState<string | null>(null);
  // Spec 0018, Abschnitt 4: leer = unverändert (bei Update), separater
  // "Entfernen"-Weg für einen bereits gesetzten Wert (s. `handleClearSudoPassword`).
  const [sudoPassword, setSudoPassword] = useState("");
  const [hasSudoPassword, setHasSudoPassword] = useState(false);
  // Spec 0071, A14/I4: Der Schlüsselbund konnte nicht sagen, ob ein
  // Sudo-Passwort hinterlegt ist. "Unbekannt" ist nicht "nein" — die
  // Oberfläche darf in diesem Fall keine der beiden Aussagen treffen.
  const [sudoPasswordUnknown, setSudoPasswordUnknown] = useState(false);
  const [clearingSudoPassword, setClearingSudoPassword] = useState(false);
  const [postIngestPolicy, setPostIngestPolicy] = useState<PostIngestPolicy>("balanced");
  const [aiInjectionCheckEnabled, setAiInjectionCheckEnabled] = useState(false);
  // Spec 0067, A2: leer = automatisch erkennen.
  const [sftpServerPath, setSftpServerPath] = useState("");
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
  const [deletePreview, setDeletePreview] = useState<DeleteServerResult | null>(null);
  // Spec 0071, A17: Der Server ist gelöscht, aber mindestens ein Secret
  // konnte nicht aus dem Schlüsselbund entfernt werden. Die Einträge sind
  // jetzt verwaist (die Server-ID gibt es nicht mehr) — das muss der
  // Nutzer erfahren, bevor die Maske zugeht.
  const [secretsLeftBehind, setSecretsLeftBehind] = useState<string[] | null>(null);

  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<TestConnectionResult | null>(null);
  const [pendingHostKey, setPendingHostKey] = useState<HostKeyInfo | null>(null);

  const [preview, setPreview] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);

  const loadServer = () => {
    setError(null);
    setTestResult(null);
    setPendingHostKey(null);
    setPreview(null);
    setDeletePreview(null);
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
        // Spec 0076, B-4: der Pfad kommt vorbefüllt aus dem DTO — anders
        // als bei den übrigen Anmeldearten reicht `authStateFromKind`
        // allein nicht, das leere Formularfeld bräuchte sonst erst einen
        // manuellen Klick auf "Datei wählen", obwohl der Server längst
        // einen Pfad hat.
        setAuth(
          server.authKind === "identity_file"
            ? { kind: "identityFile", path: server.identityFilePath ?? "", passphrase: "" }
            : authStateFromKind(server.authKind === "private_key" ? "privateKey" : server.authKind),
        );
        setSudoPassword("");
        setHasSudoPassword(server.hasSudoPassword);
        setSudoPasswordUnknown(server.sudoPasswordUnknown);
        setLocalNotes(server.notes);
        setPostIngestPolicy(server.postIngestPolicy);
        setAiInjectionCheckEnabled(server.aiInjectionCheckEnabled);
        setSftpServerPath(server.sftpServerPath ?? "");
      })
      .catch((err) => setError(commandErrorMessage(err)));
  };

  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(loadServer, [serverId, defaultGroupId]);

  // Spec 0076, B-3: fragt `inspect_key_file` für den Pfad an, der gerade im
  // Formular steht — entkoppelt, gedämpft (400 ms), damit nicht jeder
  // Tastenanschlag einen eigenen Aufruf auslöst. Läuft nur, solange die
  // Anmeldeart "Schlüsseldatei" ist; ein leerer Pfad ruft gar nicht erst an
  // (die Datei darf erst später entstehen, B-3).
  const identityPath = auth.kind === "identityFile" ? auth.path : "";
  useEffect(() => {
    if (auth.kind !== "identityFile" || identityPath.trim() === "") {
      setIdentityFacts(null);
      setIdentityFactsLoading(false);
      return;
    }
    let cancelled = false;
    setIdentityFactsLoading(true);
    const timer = setTimeout(() => {
      inspectKeyFile(identityPath)
        .then((facts) => {
          if (!cancelled) setIdentityFacts(facts);
        })
        .catch(() => {
          if (!cancelled) setIdentityFacts(null);
        })
        .finally(() => {
          if (!cancelled) setIdentityFactsLoading(false);
        });
    }, 400);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [auth.kind, identityPath]);

  // Spec 0076, C-7: derselbe Befund für den GESPEICHERTEN Pfad — unabhängig
  // davon, was der Nutzer gerade im Formular entwirft (der Knopf wirkt auf
  // den Server in der Datenbank, nicht auf den Entwurf).
  useEffect(() => {
    if (!loaded || loaded.authKind !== "identity_file" || !loaded.identityFilePath) {
      setConvertFacts(null);
      return;
    }
    let cancelled = false;
    inspectKeyFile(loaded.identityFilePath)
      .then((facts) => {
        if (!cancelled) setConvertFacts(facts);
      })
      .catch(() => {
        if (!cancelled) setConvertFacts(null);
      });
    return () => {
      cancelled = true;
    };
  }, [loaded]);

  // spec-reviewer-Fund (Spec 0058, Review des Politur-Pakets): der neue
  // "Jetzt zusammenfassen"-Link (unten) kann eine Zustimmung auslösen,
  // WÄHREND dieses Formular noch offen ist — ohne Neuladen würde ein
  // anschließender Klick auf "Speichern" die gerade akzeptierte
  // Zusammenfassung mit dem alten `localNotes`-Entwurf überschreiben.
  useEffect(() => {
    if (!loaded) return;
    const unlisten = onNoteShrinkSucceeded((event) => {
      if (event.serverId === loaded.id) loadServer();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded?.id]);

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
    // Unabhängiger Review-Pass (Spec 0039): NICHT zusätzlich mit
    // `secondOpinionAvailable` verknüpfen — sonst würde das Speichern eines
    // Servers aus einem völlig anderen Grund (z. B. Umbenennen), während
    // gerade kein Zweitmeinungs-Provider konfiguriert ist, eine zuvor
    // gesetzte Einstellung still auf `false` zurücksetzen und beim späteren
    // erneuten Konfigurieren des Providers nicht automatisch wiederkommen.
    // Die Wirksamkeitsprüfung (Provider konfiguriert?) macht ohnehin das
    // Backend (`Session::injection_check_provider`); die Checkbox hier ist
    // nur bedienbar, nicht das gespeicherte Feld selbst gegated.
    aiInjectionCheckEnabled,
    sftpServerPath: sftpServerPath.trim() === "" ? null : sftpServerPath.trim(),
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
      setSudoPasswordUnknown(false);
    } catch (err) {
      // Spec 0071, A17 (spec-reviewer-Fund): Der generische
      // KEYCHAIN_UNAVAILABLE-Text spricht von „speichern oder lesen" — das
      // Entscheidende auf diesem Pfad sagt er nicht. Der Nutzer muss
      // wissen, dass das Passwort weiter im Schlüsselbund liegt und beim
      // nächsten `sudo` erneut eingespeist wird; genau das ist die
      // Begründung, aus der A17 diesen Fall überhaupt scheitern lässt.
      setError(
        t("serverForm.removeSudoPasswordFailed") +
          " " +
          translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)),
      );
    } finally {
      setClearingSudoPassword(false);
    }
  };

  // Spec 0076, C-1/C-3 (BL-0222): der eigentliche Übernahme-Aufruf, erst
  // nach dem Bestätigungsdialog aus C-2. Bei Erfolg wird der Server neu
  // geladen (die Anmeldeart hat sich geändert, `loadServer` zeigt jetzt
  // "Private Key" statt "Schlüsseldatei") und `onSaved` informiert den
  // Aufrufer wie nach jedem anderen Speichern.
  const handleConvertToKeychain = async () => {
    if (!serverId) return;
    setConverting(true);
    setConvertError(null);
    try {
      await convertIdentityFileToKeychain(serverId);
      setConvertConfirmOpen(false);
      loadServer();
      onSaved();
    } catch (err) {
      setConvertError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    } finally {
      setConverting(false);
    }
  };

  // Spec 0046, Fund 1: analog zu `GroupForm`s zweistufigem
  // `deleteGroup`-Ablauf — der erste Aufruf (`confirm: false`) löscht
  // nichts, sondern liefert nur die Vorschau (welche Keychain-Secrets
  // entfernt würden, welche anderen Server ihre Jump-Host-Referenz
  // verlieren).
  const handleDeleteClick = async () => {
    if (!serverId) return;
    setError(null);
    try {
      setDeletePreview(await deleteServer(serverId, false));
    } catch (err) {
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    }
  };

  const handleConfirmDelete = async () => {
    if (!serverId) return;
    setDeleting(true);
    setError(null);
    try {
      const result = await deleteServer(serverId, true);
      // Spec 0071, A17: Das Löschen ist bewusst durchgelaufen. Gab es
      // Rückstände, nicht stillschweigend schließen — sonst wäre das
      // genau das falsche Erfolgssignal, das die X6-Korrektur meint.
      if (result.secretsLeftBehind.length > 0) {
        setDeletePreview(null);
        setSecretsLeftBehind(result.secretsLeftBehind);
        return;
      }
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
          {isLocalNoteLarge && (
            <div className="mt-1 rounded border border-amber-800/50 bg-amber-950/30 px-3 py-2 text-xs text-amber-300">
              <p>{t("serverForm.largeNoteHint")}</p>
              <button
                type="button"
                onClick={handleSummarizeLocalNoteNow}
                disabled={localShrinkRequesting}
                className="mt-1.5 underline hover:no-underline disabled:opacity-50"
              >
                {localShrinkRequesting ? t("serverForm.summarizeRequesting") : t("serverForm.summarizeNow")}
              </button>
              {localShrinkError && <p className="mt-1 text-red-400">{localShrinkError}</p>}
            </div>
          )}
          <textarea
            ref={localNotesRef}
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
            <option value="identityFile">{AUTH_KIND_LABELS.identityFile}</option>
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
                {/* Spec 0069, Teil C2 (BL-0110): reiner Hinweistext — keine
                 * Prüfung des Inhalts, keine Sperre, kein Einfluss auf
                 * Speichern/Verbinden. RSA-Schlüssel funktionieren
                 * unverändert weiter. */}
                <p className="mt-1 text-xs text-slate-500">{t("serverForm.ed25519Recommendation")}</p>
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

          {auth.kind === "identityFile" && (
            <div className="space-y-2">
              <div>
                <div className="flex items-center justify-between">
                  <span className="text-sm text-slate-300">{t("serverForm.identityFilePathLabel")}</span>
                  <button
                    type="button"
                    onClick={async () => {
                      const path = await pickFilePath(t("serverForm.chooseIdentityFileDialogTitle"));
                      if (path !== null) setAuth({ ...auth, path });
                    }}
                    className="rounded bg-slate-700 px-2 py-0.5 text-xs hover:bg-slate-600"
                  >
                    {t("serverForm.chooseFile")}
                  </button>
                </div>
                <input
                  type="text"
                  required
                  value={auth.path}
                  onChange={(e) => setAuth({ ...auth, path: e.target.value })}
                  placeholder={t("serverForm.identityFilePathPlaceholder")}
                  className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 font-mono text-xs text-slate-100"
                />
                {/* Spec 0076, B-3: Vorab-Befund — hindert das Speichern
                 * nicht, die Datei darf erst später entstehen. */}
                <IdentityFileFacts
                  loading={identityFactsLoading}
                  facts={identityFacts}
                  emptyPath={auth.path.trim() === ""}
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
        </fieldset>

        {/* Spec 0076, C-1/C-2/C-7 (BL-0222): nur für einen bereits
         * gespeicherten Server, dessen Anmeldeart tatsächlich (nicht nur im
         * Entwurf) eine Schlüsseldatei ist — der Knopf wirkt auf den
         * gespeicherten Server, nicht auf den gerade offenen Entwurf. */}
        {!isCreate && loaded && loaded.authKind === "identity_file" && (
          <fieldset className="rounded border border-slate-700 p-3">
            <legend className="px-1 text-sm text-slate-300">
              {t("serverForm.convertToKeychain.legend")}
            </legend>
            <p className="mb-2 text-xs text-slate-500">{t("serverForm.convertToKeychain.hint")}</p>
            {!convertConfirmOpen && (
              <>
                <button
                  type="button"
                  onClick={() => setConvertConfirmOpen(true)}
                  disabled={!convertFacts?.validKey}
                  className="rounded bg-slate-800 px-3 py-1.5 text-sm hover:bg-slate-700 disabled:opacity-50"
                >
                  {t("serverForm.convertToKeychain.button")}
                </button>
                {!convertFacts?.validKey && (
                  <p className="mt-2 text-xs text-amber-400">
                    {convertFacts
                      ? translateErrorCode(
                          t,
                          convertFacts.problem?.code,
                          convertFacts.problem?.message ?? t("serverForm.convertToKeychain.checking"),
                        )
                      : t("serverForm.convertToKeychain.checking")}
                  </p>
                )}
              </>
            )}
            {convertConfirmOpen && (
              <div className="rounded border border-slate-600 bg-slate-950 p-3 text-sm">
                <p className="mb-2 text-slate-200">{t("serverForm.convertToKeychain.confirmWhatChanges")}</p>
                <p className="mb-2 text-slate-400">{t("serverForm.convertToKeychain.confirmWhatStays")}</p>
                <p className="mb-3 font-mono text-xs break-all text-slate-300">
                  {loaded.identityFilePath}
                </p>
                {convertError && <p className="mb-2 text-xs text-red-400">{convertError}</p>}
                <div className="flex gap-2">
                  <button
                    type="button"
                    onClick={() => {
                      setConvertConfirmOpen(false);
                      setConvertError(null);
                    }}
                    className="rounded bg-slate-700 px-3 py-1 text-xs hover:bg-slate-600"
                  >
                    {t("common.cancel")}
                  </button>
                  <button
                    type="button"
                    onClick={handleConvertToKeychain}
                    disabled={converting}
                    className="rounded bg-indigo-600 px-3 py-1 text-xs text-white hover:bg-indigo-500 disabled:opacity-50"
                  >
                    {converting
                      ? t("serverForm.convertToKeychain.converting")
                      : t("serverForm.convertToKeychain.confirm")}
                  </button>
                </div>
              </div>
            )}
          </fieldset>
        )}

        <fieldset className="rounded border border-slate-700 p-3">
          <legend className="px-1 text-sm text-slate-300">{t("serverForm.sudoFieldset")}</legend>
          <p className="mb-2 text-xs text-slate-500">{t("serverForm.sudoHint")}</p>
          <label className="block text-sm text-slate-300">
            {t("serverForm.sudoLabel")}{" "}
            <span className="text-slate-500">
              {sudoPasswordUnknown
                ? t("serverForm.sudoUnchangedUnknown")
                : hasSudoPassword
                  ? t("serverForm.sudoUnchangedStored")
                  : t("serverForm.sudoUnchanged")}
            </span>
            <input
              type="password"
              value={sudoPassword}
              onChange={(e) => setSudoPassword(e.target.value)}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>
          {(hasSudoPassword || sudoPasswordUnknown) && (
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

        {!isLocal && (
          <details className="rounded border border-slate-700 p-3">
            <summary className="cursor-pointer text-sm text-slate-300">
              {t("serverForm.advanced")}
            </summary>
            <label className="mt-3 block text-sm text-slate-300">
              {t("serverForm.sftpServerPathLabel")}
              <input
                value={sftpServerPath}
                onChange={(e) => setSftpServerPath(e.target.value)}
                placeholder={t("serverForm.sftpServerPathPlaceholder")}
                className="mt-1 w-full rounded border border-slate-700 bg-slate-950 px-2 py-1 font-mono text-sm text-slate-100 focus:outline-none"
              />
            </label>
            <p className="mt-2 text-xs text-slate-500">{t("serverForm.sftpServerPathHint")}</p>
          </details>
        )}

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
          <NotesPanel
            target={{ Server: serverId }}
            currentNotes={loaded.notes}
            onNotesChanged={onSaved}
            autoFocus={autoFocusNotes}
          />

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
              onClick={handleDeleteClick}
              className="rounded bg-red-900 px-3 py-1.5 text-sm text-red-200 hover:bg-red-800"
            >
              {t("serverForm.deleteServer")}
            </button>

            {/* Spec 0071, A17: Der Server ist weg, aber mindestens ein
             * Secret blieb im Schlüsselbund. Kein stilles Schließen — der
             * Nutzer braucht die Refs, um die verwaisten Einträge im
             * Schlüsselbund-Verwaltungsprogramm wiederzufinden. */}
            {secretsLeftBehind && (
              <div
                className="mt-3 rounded border border-amber-700 bg-amber-950 p-3 text-sm"
                data-testid="secrets-left-behind"
              >
                <p className="mb-2 font-medium text-amber-200">
                  {t("serverForm.deletedWithLeftoverSecretsTitle")}
                </p>
                <p className="mb-2 text-amber-200">
                  {t("serverForm.deletedWithLeftoverSecretsHint")}
                </p>
                <ul className="mb-2 space-y-1 font-mono text-xs text-amber-200">
                  {secretsLeftBehind.map((ref) => (
                    <li key={ref}>{ref}</li>
                  ))}
                </ul>
                <button
                  type="button"
                  onClick={() => {
                    setSecretsLeftBehind(null);
                    onDeleted();
                  }}
                  className="rounded bg-slate-800 px-2 py-1 text-xs text-slate-200 hover:bg-slate-700"
                >
                  {t("common.close")}
                </button>
              </div>
            )}

            {deletePreview && (
              <div className="mt-3 rounded border border-red-800 bg-red-950 p-3 text-sm">
                <p className="mb-2 font-medium text-red-200">{t("serverForm.deleteImpactTitle")}</p>
                {deletePreview.server.authKind === "agent" &&
                !deletePreview.server.hasSudoPassword &&
                !deletePreview.server.sudoPasswordUnknown &&
                deletePreview.serversLosingJumpHost.length === 0 ? (
                  <p className="mb-2 text-red-200">{t("serverForm.deleteNoKeychainImpact")}</p>
                ) : (
                  <ul className="mb-2 space-y-1 text-red-200">
                    {/* Spec 0076: `identity_file` hat keinen Secret-Inhalt
                     * im Schlüsselbund, der zwingend existiert — anders als
                     * bei den übrigen Anmeldearten steht dort höchstens
                     * eine OPTIONALE Passphrase, und das DTO sagt nicht,
                     * ob sie gesetzt ist. "Wird gelöscht" wäre hier eine
                     * Behauptung, die die Oberfläche nicht belegen kann;
                     * die eigene "mag gelöscht werden"-Zeile unten (wie bei
                     * `sudoPasswordUnknown`) bleibt deshalb der einzige
                     * Hinweis für diese Anmeldeart. */}
                    {deletePreview.server.authKind !== "agent" &&
                      deletePreview.server.authKind !== "identity_file" && (
                        <li>
                          {t("serverForm.secretWillBeDeleted", {
                            label: authKindLabels(t)[deletePreview.server.authKind],
                          })}
                        </li>
                      )}
                    {deletePreview.server.authKind === "identity_file" && (
                      <li>
                        {t("serverForm.secretMayBeDeleted", {
                          label: t("serverForm.identityFilePassphraseLabel"),
                        })}
                      </li>
                    )}
                    {deletePreview.server.hasSudoPassword && (
                      <li>
                        {t("serverForm.secretWillBeDeleted", { label: t("serverForm.sudoLabel") })}
                      </li>
                    )}
                    {/* Spec 0071, A14: Ist der Schlüsselbund nicht lesbar,
                     * darf hier weder "wird gelöscht" noch gar nichts
                     * stehen — beides wäre eine Behauptung über einen
                     * Zustand, den die Oberfläche nicht kennt. */}
                    {deletePreview.server.sudoPasswordUnknown && (
                      <li>
                        {t("serverForm.secretMayBeDeleted", { label: t("serverForm.sudoLabel") })}
                      </li>
                    )}
                    {deletePreview.serversLosingJumpHost.map((s) => (
                      <li key={s.id}>{t("serverForm.serverWillLoseJumpHost", { name: s.name })}</li>
                    ))}
                  </ul>
                )}
                {/* spec-reviewer-Fund (Review dieses Schritts): die Vorschau
                 * oben deckt nur Secrets/Jump-Host ab (Spec 0046, Fund 1) —
                 * ohne diesen Hinweis läse sich "keine Secrets, keine
                 * anderen Server betroffen" wie "löschen ist folgenlos",
                 * obwohl Chat-Historie/Notizen/Tags dieses Servers beim
                 * Löschen unwiderruflich mitverschwinden (`ON DELETE
                 * CASCADE`, Spec 0034/0008). */}
                <p className="mb-2 text-red-200">{t("serverForm.deleteAlwaysRemovesHistory")}</p>
                <div className="flex gap-2">
                  <button
                    type="button"
                    onClick={() => setDeletePreview(null)}
                    className="rounded bg-slate-700 px-3 py-1 text-xs hover:bg-slate-600"
                  >
                    {t("common.cancel")}
                  </button>
                  <button
                    type="button"
                    onClick={handleConfirmDelete}
                    disabled={deleting}
                    className="rounded bg-red-700 px-3 py-1 text-xs text-white hover:bg-red-600 disabled:opacity-50"
                  >
                    {deleting ? t("common.deleting") : t("serverForm.confirmDeleteServer")}
                  </button>
                </div>
              </div>
            )}
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

// Spec 0069, Teil A5, Test 20: exportiert (statt modulprivat), damit sich
// die code->Übersetzung ohne einen vollständigen `ServerForm`-Render
// testen lässt.
export function TestResultBadge({ result }: { result: TestConnectionResult }) {
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
      // Spec 0069, Teil A5: bekannter Code → übersetzte Meldung; ohne
      // Code → wie bisher der rohe Backend-Text.
      return (
        <span className="text-sm text-red-400">
          {translateErrorCode(
            t,
            result.code,
            t("serverForm.testResult.networkError", { message: result.message }),
          )}
        </span>
      );
    case "timeout":
      return <span className="text-sm text-red-400">{t("serverForm.testResult.timeout")}</span>;
  }
}
