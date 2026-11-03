import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ServerFormSections, ServerFormTabSections, useServerSettingsTabs } from "../extensions/FormSlots";
import { resolveSectionLabel } from "../resolveSectionLabel";
import { ConnectStepLog } from "./ConnectStepLog";
import { TestConnectionDialog } from "./TestConnectionDialog";
import { IdentityFileFacts } from "./serverForm/IdentityFileFacts";
import { TestResultBadge } from "./serverForm/TestResultBadge";
import { useConnectionTest } from "./serverForm/useConnectionTest";
import { useIdentityFile } from "./serverForm/useIdentityFile";
import { useServerDelete } from "./serverForm/useServerDelete";
import {
  clearServerSudoPassword,
  commandErrorMessage,
  convertIdentityFileToKeychain,
  createServer,
  getServer,
  largeNoteDialogThresholdChars,
  listStoredHostKeys,
  previewEffectiveNotes,
  requestNoteShrink,
  updateLocalServerNotes,
  updateLocalServerTags,
  updateServer,
} from "../api";
import { commandErrorText, translateErrorCode } from "../errorCodes";
import { onNoteShrinkSucceeded } from "../events";
import { pickAndReadTextFile, pickFilePath } from "../fileDialog";
import { flattenGroupOptions, groupOptionLabel } from "../groupTree";
import { loadRiskClassifierSettings } from "../riskSettings";
import { checkStartDirectory } from "../startDirectory";
import type {
  AuthMethodInput,
  AuthMethodKind,
  GroupDto,
  PostIngestPolicy,
  ServerDto,
  ServerInput,
  StoredHostKeyDto,
} from "../types";
import type { MovedTo } from "./GroupForm";
import { HostKeyDialog } from "./HostKeyDialog";
import { DialogFrame } from "./ui/DialogFrame";
import { detectFallbackPlatform, platformCapabilities } from "../titlebarLayout";
import { NotesPanel } from "./NotesPanel";
import { TECHNICAL_INPUT_PROPS } from "../technicalInputProps";
import { Button, ErrorBoundary, TabPanel, Tabs, tabId, useTabsIdBase, type TabItem } from "./ui";
import { Field, FIELD_CONTROL_CLASS } from "./ui/Field";

export { TestResultBadge };

interface ServerFormProps {
  /** `null` = Neuanlage. */
  serverId: string | null;
  defaultGroupId: string | null;
  allGroups: GroupDto[];
  allServers: ServerDto[];
  onSaved: () => void;
  onDeleted: () => void;
  /** Issue #692: shows "Connect" next to Save for a saved server. */
  onConnect?: (serverId: string) => void;
  /** Spec 0058, Teil 2 — s. `NotesPanel.autoFocus`-Doc-Kommentar. Gilt für
   * BEIDE Notiz-Editoren dieses Formulars (den lokalen Pseudo-Server-Zweig
   * unten und den regulären `NotesPanel`-Zweig) — "Mache ich selbst" kann
   * sich auf jeden Server beziehen, den lokalen eingeschlossen. */
  autoFocusNotes?: boolean;
  /** Issue #63: s. `MovedTo` in `GroupForm.tsx`. `null`/fehlend = nicht
   * verschoben. */
  movedTo?: MovedTo | null;
}

/** `t` wird als Parameter durchgereicht statt selbst `useTranslation()`
 * aufzurufen — diese Funktion läuft außerhalb einer Komponente, Hooks sind
 * hier nicht erlaubt. */
function authKindLabels(
  t: (key: string) => string,
): Record<AuthMethodKind | "privateKey" | "identityFile", string> {
  return {
    password: t("serverForm.authKind.password"),
    no_password: t("serverForm.authKind.password"),
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
  // `saved`: der Server hat ein gespeichertes Passwort (Issue #632) — nur
  // dann bedeutet ein leeres Feld "unverändert" und gibt es "entfernen".
  | { kind: "password"; value: string; saved: boolean; remove: boolean }
  | { kind: "privateKey"; keyContent: string; passphrase: string }
  | { kind: "agent" }
  | { kind: "certificate"; certContent: string; keyContent: string }
  | { kind: "identityFile"; path: string; passphrase: string };

function authStateFromKind(kind: AuthFormState["kind"]): AuthFormState {
  switch (kind) {
    case "password":
      return { kind: "password", value: "", saved: false, remove: false };
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
 *
 * Spec 0073, A6–A8 (Q-BL-0149-02): Derselbe `buildInput()` bedient auch
 * "Verbindung testen" (s. unten) — und dort gilt seither dieselbe Regel.
 * Ein leeres Pflichtfeld bei Neuanlage endet also in beiden Fällen in der
 * Fehlermeldung, nicht in einem Anmeldeversuch mit leerem Secret. Auf
 * Randzeichen muss dieses Formular bei den **Secret-Feldern** deshalb
 * nicht prüfen: Beide Wege trimmen im Kern über denselben Helfer, und
 * zwar bevor sie "leer" entscheiden. Für `identityFile.path` gilt das
 * ausdrücklich **nicht** — der Pfad wird bewusst nicht getrimmt
 * (ADR 0065 §5, s. Kommentar am Zweig unten).
 */
function toAuthMethodInput(state: AuthFormState, isCreate: boolean): AuthMethodInput {
  const orNullIfUpdate = (value: string) => (value === "" && !isCreate ? null : value);
  switch (state.kind) {
    case "password":
      // Issue #632: Das Passwort ist optional. "Entfernen" und ein leeres
      // Feld ohne gespeichertes Passwort sind ausdrücklich "kein Passwort";
      // "unveränderte" (`null`) gibt es nur bei einem gespeicherten.
      if (state.remove || (state.value === "" && (isCreate || !state.saved))) {
        return { kind: "noPassword" };
      }
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

export function ServerForm({
  serverId,
  defaultGroupId,
  allGroups,
  allServers,
  onSaved,
  onDeleted,
  onConnect,
  autoFocusNotes = false,
  movedTo = null,
}: ServerFormProps) {
  const { t, i18n } = useTranslation();
  const AUTH_KIND_LABELS = authKindLabels(t);
  const isCreate = serverId === null;

  const [loaded, setLoaded] = useState<ServerDto | null>(null);
  // Spec 0038, Abschnitt 4: ungespeicherte Änderungen — für Abschnitte, die
  // Erweiterungen im Formular registrieren.
  const [dirty, setDirty] = useState(false);
  // Spec 0032, Abschnitt 3: nur bekannt, sobald `loaded` geladen ist (der
  // lokale Pseudo-Server ist nie `serverId === null`, also nie `isCreate`).
  const isLocal = loaded?.isLocal ?? false;
  // Spec 0008: stored host keys of the saved host/port; null = not loaded
  // (or load failed), shown only for an existing, non-local server.
  const [storedHostKeys, setStoredHostKeys] = useState<StoredHostKeyDto[] | null>(null);
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
    largeNoteDialogThresholdChars()
      .then(setLargeLocalNoteThreshold)
      .catch((err) => console.error(commandErrorMessage(err)));
  }, []);
  // Spec 0079, A4: Unicode-Skalarwerte (`[...text].length`), nicht Byte.
  const isLocalNoteLarge =
    largeLocalNoteThreshold !== null && [...localNotes].length >= largeLocalNoteThreshold;
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
  const [auth, setAuth] = useState<AuthFormState>(authStateFromKind("password"));
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
  // Spec 0102: leer = Home des Login-Nutzers.
  const [startDirectory, setStartDirectory] = useState("");
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

  // Issue #606: the form is split into tabs. All panels stay mounted (hidden),
  // so one draft, the refs and the connection-test log survive tab switches.
  type ServerTab = "general" | "connection" | "security" | "notes";
  const tabsIdBase = useTabsIdBase();
  const formRef = useRef<HTMLFormElement>(null);
  const [requestedTab, setActiveTab] = useState<string>(autoFocusNotes && !isCreate ? "notes" : "general");
  // Issue #608: extension tabs (saved servers only, local pseudo-server included).
  const extensionTabs = useServerSettingsTabs();
  const [openedExtTabs, setOpenedExtTabs] = useState<ReadonlySet<string>>(new Set());
  const [invalidTabs, setInvalidTabs] = useState<ReadonlySet<ServerTab>>(new Set());
  const [reportValidityPending, setReportValidityPending] = useState(false);
  // Selecting another server opens General again (derived state, no effect).
  const [tabsServerId, setTabsServerId] = useState(serverId);
  if (tabsServerId !== serverId) {
    setTabsServerId(serverId);
    setActiveTab("general");
    setInvalidTabs(new Set());
    setOpenedExtTabs(new Set());
  }
  const availableTabs: ServerTab[] = isLocal
    ? ["general", "notes"]
    : isCreate
      ? ["general", "connection", "security"]
      : ["general", "connection", "security", "notes"];
  const shownExtTabs = !isCreate && loaded ? extensionTabs : [];
  const tabItems: TabItem[] = [
    ...availableTabs.map((id) => ({
      id: id as string,
      label: id === "notes" ? t("common.notes") : t(`serverForm.tabs.${id}`),
      invalid: invalidTabs.has(id),
      invalidLabel: t("serverForm.tabs.invalid"),
    })),
    ...shownExtTabs.map((x) => ({
      id: `ext:${x.id}`,
      label: resolveSectionLabel(x.label, x.id, i18n, t),
    })),
  ];
  // An unregistered active extension tab falls back to General.
  const activeTab = tabItems.some((item) => item.id === requestedTab) ? requestedTab : "general";
  if (activeTab.startsWith("ext:") && !openedExtTabs.has(activeTab)) {
    setOpenedExtTabs(new Set(openedExtTabs).add(activeTab));
  }
  // Lazily mounted when first opened, mounted while the server stays selected.
  const renderExtensionPanels = (extServerId: string) =>
    shownExtTabs
    .filter((x) => openedExtTabs.has(`ext:${x.id}`))
    .map((x) => (
      <TabPanel
        key={x.id}
        idBase={tabsIdBase}
        id={`ext:${x.id}`}
        active={activeTab === `ext:${x.id}`}
        className="space-y-6"
      >
        <ErrorBoundary id={`server-settings-tab:${x.id}`} resetKeys={[extServerId]}><x.component serverId={extServerId} /></ErrorBoundary>
      </TabPanel>
    ));
  // Tabs whose fields fail the browser validation (the form is `noValidate`,
  // so that an invalid field in a hidden panel cannot silently block saving).
  const collectInvalidTabs = (): Set<ServerTab> => {
    const found = new Set<ServerTab>();
    const panels = formRef.current?.querySelectorAll<HTMLElement>('[role="tabpanel"]') ?? [];
    for (const panel of panels) {
      const tab = (["general", "connection", "security"] as const).find((id) =>
        panel.id.endsWith(`-panel-${id}`),
      );
      if (!tab) continue;
      for (const el of panel.querySelectorAll<HTMLInputElement>("input, select, textarea")) {
        if (!el.checkValidity()) {
          found.add(tab);
          break;
        }
      }
    }
    return found;
  };
  useEffect(() => {
    if (!reportValidityPending) return;
    setReportValidityPending(false);
    formRef.current?.reportValidity();
  }, [reportValidityPending, activeTab]);
  const [preview, setPreview] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);

  const loadServer = () => {
    setError(null);
    resetResult();
    setPreview(null);
    setDeletePreview(null);
    // spec-reviewer-Fund (Review dieses Schritts): ohne diesen Reset hinge
    // die C-2-Bestätigungsgrenze allein an `ManagementView`s `key={id}`
    // (das `ServerForm` bei jedem Serverwechsel neu montiert) — richtig,
    // aber eine Eigenschaft des Aufrufers, nicht der Komponente selbst.
    // `convertFacts`/-`Error` reagieren bereits über den eigenen Effekt auf
    // `loaded`; nur diese drei bräuchten sonst einen fremden Anker.
    // spec-reviewer-Fund (Review Runde 1, verifiziert in Runde 2): ohne
    // diesen Reset hinge die C-2-Bestätigungsgrenze allein an
    // `ManagementView`s `key={id}` (das `ServerForm` bei jedem
    // Serverwechsel neu montiert) — richtig, aber eine Eigenschaft des
    // Aufrufers, nicht der Komponente selbst. `convertFacts`/-`Error`
    // reagieren bereits über den eigenen Effekt auf `loaded`; nur diese
    // drei bräuchten sonst einen fremden Anker.
    resetConvert();
    setStoredHostKeys(null);
    if (serverId === null) {
      setLoaded(null);
      setName("");
      setHost("");
      setPort("22");
      setUsername("");
      setGroupId(defaultGroupId);
      setJumpHost(null);
      setTags([]);
      setAuth(authStateFromKind("password"));
      setSudoPassword("");
      setHasSudoPassword(false);
      setPostIngestPolicy("balanced");
      setAiInjectionCheckEnabled(false);
      return;
    }
    listStoredHostKeys(serverId)
      .then((keys) => setStoredHostKeys(keys))
      .catch(() => setStoredHostKeys(null));
    getServer(serverId)
      .then((server) => {
        setLoaded(server);
        setDirty(false);
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
            : server.authKind === "password" || server.authKind === "no_password"
              ? {
                  kind: "password",
                  value: "",
                  saved: server.authKind === "password",
                  remove: false,
                }
              : authStateFromKind(server.authKind === "private_key" ? "privateKey" : server.authKind),
        );
        setSudoPassword("");
        setHasSudoPassword(server.hasSudoPassword);
        setSudoPasswordUnknown(server.sudoPasswordUnknown);
        setLocalNotes(server.notes);
        setPostIngestPolicy(server.postIngestPolicy);
        setAiInjectionCheckEnabled(server.aiInjectionCheckEnabled);
        setSftpServerPath(server.sftpServerPath ?? "");
        setStartDirectory(server.startDirectory ?? "");
      })
      .catch((err) => setError(commandErrorMessage(err)));
  };

  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(loadServer, [serverId, defaultGroupId]);

  // Issue #63: nach einem erfolgreichen Verschieben dieses Servers per
  // Drag-and-drop nur die Gruppe übernehmen — alle übrigen ungespeicherten
  // Eingaben bleiben stehen. Ohne das stünde hier noch die alte Gruppe,
  // und ein späteres Speichern machte das Verschieben rückgängig.
  useEffect(() => {
    if (movedTo) setGroupId(movedTo.groupId);
  }, [movedTo]);

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
    // Spec 0102: der lokale Pseudo-Server bietet das Feld nicht an.
    startDirectory: isLocal || !startDirectoryCheck.ok ? null : startDirectoryCheck.value,
  });

  const startDirectoryCheck = checkStartDirectory(startDirectory);
  const startDirectoryError = startDirectoryCheck.ok
    ? null
    : t(`serverForm.startDirectoryInvalid.${startDirectoryCheck.reason}`);

  const {
    testing,
    testResult,
    testDialog,
    pendingHostKey,
    setPendingHostKey,
    resetResult,
    runTest,
    cancelTest,
    closeTestDialog,
    handleTrustAndRetest,
  } = useConnectionTest({ serverId, buildInput, setError });
  const { deleting, deletePreview, secretsLeftBehind, setDeletePreview, setSecretsLeftBehind, handleDeleteClick, handleConfirmDelete } =
    useServerDelete({ serverId, setError, onDeleted });
  const {
    identityFacts,
    identityFactsLoading,
    identityFactsError,
    convertFacts,
    convertFactsError,
    convertConfirmOpen,
    setConvertConfirmOpen,
    converting,
    convertError,
    setConvertError,
    resetConvert,
    handleConvertToKeychain,
    importPromptOpen,
    setImportPromptOpen,
    importError,
    setImportError,
    importSavedId,
    setImportSavedId,
    finishKeepingPath,
  } = useIdentityFile({
    serverId,
    draftIsIdentityFile: auth.kind === "identityFile",
    draftPath: auth.kind === "identityFile" ? auth.path : "",
    loaded,
    reloadServer: () => loadServer(),
    onSaved,
    onKeepPathFinished: () => {
      setDirty(false);
      setInvalidTabs(new Set());
      onSaved();
    },
  });

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const invalid = collectInvalidTabs();
    if (!isLocal && startDirectoryError) invalid.add("connection");
    setInvalidTabs(invalid);
    if (invalid.size > 0) {
      const first = availableTabs.find((id) => invalid.has(id));
      if (first) setActiveTab(first);
      if (!isLocal && startDirectoryError) setError(startDirectoryError);
      setReportValidityPending(true);
      return;
    }
    // Issue #667: a key-file server that is new or whose path changed asks
    // once whether to import the key or keep using the file path.
    if (
      auth.kind === "identityFile" &&
      auth.path.trim() !== "" &&
      !(loaded?.authKind === "identity_file" && loaded.identityFilePath === auth.path)
    ) {
      setImportError(null);
      setImportSavedId(null);
      setImportPromptOpen(true);
      return;
    }
    await save(false);
  };

  /** Saves the form; with `importKey` the key file is then imported through
   * the existing conversion (`convertIdentityFileToKeychain`). */
  const save = async (importKey: boolean) => {
    setSaving(true);
    setError(null);
    setImportError(null);
    let savedId = importSavedId;
    try {
      if (savedId === null) {
        if (isCreate) {
          savedId = await createServer(buildInput());
        } else if (serverId) {
          await updateServer(serverId, buildInput());
          savedId = serverId;
        }
      }
      if (importKey && savedId) {
        setImportSavedId(savedId);
        await convertIdentityFileToKeychain(savedId);
      }
      setImportPromptOpen(false);
      setImportSavedId(null);
      setDirty(false);
      setInvalidTabs(new Set());
      onSaved();
    } catch (err) {
      const message = commandErrorText(t, err);
      // The server itself is saved once `importSavedId` is set; only the
      // import failed — keep the dialog open for a retry or "keep path".
      if (importKey && savedId !== null) {
        setImportError(message);
      } else {
        setError(message);
        setImportPromptOpen(false);
      }
    } finally {
      setSaving(false);
    }
  };

  const importPrimaryRef = useRef<HTMLButtonElement>(null);

  // Cancel/Escape of the key-import prompt; after a failed import the server is already saved, so it acts like "keep".
  const cancelImportPrompt = () => {
    if (saving) return;
    if (importSavedId !== null) finishKeepingPath();
    else setImportPromptOpen(false);
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
      setError(t("serverForm.removeSudoPasswordFailed") + " " + commandErrorText(t, err));
    } finally {
      setClearingSudoPassword(false);
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
      setDirty(false);
      onSaved();
    } catch (err) {
      setError(commandErrorText(t, err));
    } finally {
      setSavingLocalTags(false);
    }
  };

  const handleSaveLocalNotes = async () => {
    setSavingLocalNotes(true);
    setError(null);
    try {
      await updateLocalServerNotes(localNotes);
      setDirty(false);
      onSaved();
    } catch (err) {
      setError(commandErrorText(t, err));
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
      setError(commandErrorText(t, err));
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
      <div className="max-w-2xl space-y-6 p-4" onChange={() => setDirty(true)}>
        <h2 className="font-heading text-lg font-semibold tracking-wide text-fg">{loaded.name}</h2>
        <p className="text-sm text-fg-subtle">{t("serverForm.localHint")}</p>

        {error && <p className="text-sm text-danger-fg">{error}</p>}

        <Tabs
          tabs={tabItems}
          value={activeTab}
          onChange={(id) => setActiveTab(id as ServerTab)}
          ariaLabel={t("serverForm.tabs.label")}
          idBase={tabsIdBase}
        />

        <TabPanel idBase={tabsIdBase} id="general" active={activeTab === "general"} className="space-y-6">
        <div className="block text-sm text-fg-muted">
          {t("serverForm.tags")}
          <div className="mt-1 flex flex-wrap items-center gap-1 rounded border border-border-strong bg-surface p-1.5">
            {tags.map((tag) => (
              <span
                key={tag}
                className="flex items-center gap-1 rounded bg-border px-2 py-0.5 text-xs text-fg"
              >
                {tag}
                <button
                  type="button"
                  onClick={() => {
                    setDirty(true);
                    setTags(tags.filter((tagValue) => tagValue !== tag));
                  }}
                  className="rounded text-fg-subtle hover:text-fg-bright focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
                  aria-label={t("serverForm.removeTagAria", { tag })}
                >
                  ✕
                </button>
              </span>
            ))}
            <input
              {...TECHNICAL_INPUT_PROPS}
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
              className="min-w-24 flex-1 rounded bg-transparent px-1 py-0.5 text-sm text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
            />
          </div>
          <Button variant="primary" size="sm"
            onClick={handleSaveLocalTags}
            disabled={savingLocalTags}
            className="mt-2"
          >
            {savingLocalTags ? t("common.saving") : t("common.save")}
          </Button>
        </div>

        <ServerFormSections serverId={loaded.id} dirty={dirty} tab="general" />
        </TabPanel>

        <TabPanel idBase={tabsIdBase} id="notes" active={activeTab === "notes"} className="space-y-6">
        <div className="block text-sm text-fg-muted">
          {t("common.notes")}
          {isLocalNoteLarge && (
            <div className="mt-1 rounded border border-warning-border/50 bg-warning-subtle/30 px-3 py-2 text-xs text-warning-fg-soft">
              <p>{t("serverForm.largeNoteHint")}</p>
              <button
                type="button"
                onClick={handleSummarizeLocalNoteNow}
                disabled={localShrinkRequesting}
                className="mt-1.5 underline hover:no-underline disabled:opacity-50"
              >
                {localShrinkRequesting ? t("serverForm.summarizeRequesting") : t("serverForm.summarizeNow")}
              </button>
              {localShrinkError && <p className="mt-1 text-danger-fg">{localShrinkError}</p>}
            </div>
          )}
          <textarea
            ref={localNotesRef}
            value={localNotes}
            onChange={(e) => setLocalNotes(e.target.value)}
            rows={6}
            className="mt-1 w-full rounded border border-border-strong bg-surface px-2 py-1.5 text-sm text-fg"
          />
          <Button variant="primary" size="sm"
            onClick={handleSaveLocalNotes}
            disabled={savingLocalNotes}
            className="mt-2"
          >
            {savingLocalNotes ? t("common.saving") : t("common.save")}
          </Button>
        </div>
        <ServerFormSections serverId={loaded.id} dirty={dirty} tab="notes" />
        </TabPanel>
        {renderExtensionPanels(loaded.id)}
      </div>
    );
  }

  return (
    <div className="max-w-2xl space-y-6 p-4">
      <h2 className="font-heading text-lg font-semibold tracking-wide text-fg">
        {isCreate ? t("serverForm.titleNew") : t("serverForm.titleExisting", { name: loaded?.name ?? "" })}
      </h2>

      <Tabs
        tabs={tabItems}
        value={activeTab}
        onChange={(id) => setActiveTab(id as ServerTab)}
        ariaLabel={t("serverForm.tabs.label")}
        idBase={tabsIdBase}
      />

      <form
        ref={formRef}
        noValidate
        onSubmit={handleSubmit}
        onChange={() => {
          setDirty(true);
          if (invalidTabs.size > 0) setInvalidTabs(collectInvalidTabs());
        }}
        className="space-y-3"
      >
        <TabPanel idBase={tabsIdBase} id="general" active={activeTab === "general"} className="space-y-3">
          <Field label={t("common.name")}>
            {(control) => (
              <input
                {...control}
                {...TECHNICAL_INPUT_PROPS}
                type="text"
                required
                value={name}
                onChange={(e) => setName(e.target.value)}
                className={`${FIELD_CONTROL_CLASS} w-full`}
              />
            )}
          </Field>

        <Field label={t("serverForm.group")}>
          {(control) => (
            <select
              {...control}
              value={groupId ?? ""}
              onChange={(e) => setGroupId(e.target.value || null)}
              className={`${FIELD_CONTROL_CLASS} w-full`}
            >
              <option value="">{t("serverForm.noGroup")}</option>
              {/* Issue #49: hierarchisch (eingerückter voller Pfad), damit
               * eine vorbelegte Untergruppe erkennbar ist. */}
              {flattenGroupOptions(allGroups).map((option) => (
                <option key={option.group.id} value={option.group.id}>
                  {groupOptionLabel(option)}
                </option>
              ))}
            </select>
          )}
        </Field>

        <div className="block text-sm text-fg-muted">
          {t("serverForm.tags")}
          <div className="mt-1 flex flex-wrap items-center gap-1 rounded border border-border-strong bg-surface p-1.5">
            {tags.map((tag) => (
              <span
                key={tag}
                className="flex items-center gap-1 rounded bg-border px-2 py-0.5 text-xs text-fg"
              >
                {tag}
                <button
                  type="button"
                  onClick={() => setTags(tags.filter((tagValue) => tagValue !== tag))}
                  className="rounded text-fg-subtle hover:text-fg-bright focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
                  aria-label={t("serverForm.removeTagAria", { tag })}
                >
                  ✕
                </button>
              </span>
            ))}
            <input
              {...TECHNICAL_INPUT_PROPS}
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
              className="min-w-24 flex-1 rounded bg-transparent px-1 py-0.5 text-sm text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
            />
          </div>
        </div>
        </TabPanel>

        <TabPanel idBase={tabsIdBase} id="connection" active={activeTab === "connection"} className="space-y-3">
        <div className="grid grid-cols-2 gap-3 narrow:grid-cols-1">
          <Field label={t("serverForm.host")}>
            {(control) => (
              <input
                {...control}
                {...TECHNICAL_INPUT_PROPS}
                type="text"
                required
                value={host}
                onChange={(e) => setHost(e.target.value)}
                className={`${FIELD_CONTROL_CLASS} w-full`}
              />
            )}
          </Field>
          <Field label={t("serverForm.port")}>
            {(control) => (
              <input
                {...control}
                type="number"
                required
                min={1}
                max={65535}
                value={port}
                onChange={(e) => setPort(e.target.value)}
                className={`${FIELD_CONTROL_CLASS} w-full`}
              />
            )}
          </Field>
          <Field label={t("serverForm.username")}>
            {(control) => (
              <input
                {...control}
                {...TECHNICAL_INPUT_PROPS}
                type="text"
                required
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                className={`${FIELD_CONTROL_CLASS} w-full`}
              />
            )}
          </Field>
        </div>

        <Field label={t("serverForm.jumpHost")}>
          {(control) => (
            <select
              {...control}
              value={jumpHost ?? ""}
              onChange={(e) => setJumpHost(e.target.value || null)}
              className={`${FIELD_CONTROL_CLASS} w-full`}
            >
              <option value="">{t("serverForm.noJumpHost")}</option>
              {possibleJumpHosts.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
          )}
        </Field>

        <fieldset className="rounded border border-border p-3">
          <legend className="px-1 text-sm text-fg-muted">{t("serverForm.authFieldset")}</legend>
          <select
            aria-label={t("serverForm.authFieldset")}
            value={auth.kind}
            onChange={(e) => setAuth(authStateFromKind(e.target.value as AuthFormState["kind"]))}
            className="mb-3 w-full rounded border border-border-strong bg-surface px-2 py-1.5 text-fg"
          >
            <option value="password">{AUTH_KIND_LABELS.password}</option>
            <option value="privateKey">{AUTH_KIND_LABELS.privateKey}</option>
            <option value="agent">{AUTH_KIND_LABELS.agent}</option>
            <option value="certificate">{AUTH_KIND_LABELS.certificate}</option>
            {/* Issue #679: a file path cannot be kept on Android (the picker
             * returns a content URI), so "Key file" is only offered for a
             * server that already uses it; new keys go in via "Private key",
             * which imports the picked content. */}
            {(platformCapabilities(detectFallbackPlatform()).keyFilePath ||
              auth.kind === "identityFile") && (
              <option value="identityFile">{AUTH_KIND_LABELS.identityFile}</option>
            )}
          </select>

          {auth.kind === "password" && (
            <div className="space-y-2">
              <Field
                label={
                  <>
                    {t("serverForm.password")}{" "}
                    <span className="text-fg-subtle">
                      {!isCreate && auth.saved
                        ? t("serverForm.unchangedHint")
                        : t("serverForm.passwordOptionalHint")}
                    </span>
                  </>
                }
              >
                {(control) => (
                  <input
                    {...control}
                    {...TECHNICAL_INPUT_PROPS}
                    type="password"
                    disabled={auth.remove}
                    value={auth.value}
                    onChange={(e) => setAuth({ ...auth, value: e.target.value })}
                    className={`${FIELD_CONTROL_CLASS} w-full`}
                  />
                )}
              </Field>
              {!isCreate && auth.saved && (
                <label className="flex items-center gap-2 text-sm text-fg-muted">
                  <input
                    type="checkbox"
                    checked={auth.remove}
                    onChange={(e) => setAuth({ ...auth, remove: e.target.checked, value: "" })}
                  />
                  {t("serverForm.removeSavedPassword")}
                </label>
              )}
              {!isCreate && !auth.saved && loaded?.authKind === "no_password" && (
                <p className="text-xs text-fg-subtle">{t("serverForm.noPasswordSaved")}</p>
              )}
            </div>
          )}

          {auth.kind === "privateKey" && (
            <div className="space-y-2">
              <div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="text-sm text-fg-muted">
                    {t("serverForm.privateKeyLabel")}{" "}
                    {!isCreate && <span className="text-fg-subtle">{t("serverForm.unchangedHint")}</span>}
                  </span>
                  <Button size="sm"
                    onClick={async () => {
                      const content = await pickAndReadTextFile(
                        t("serverForm.choosePrivateKeyDialogTitle"),
                      );
                      if (content !== null) setAuth({ ...auth, keyContent: content });
                    }}
                  >
                    {t("serverForm.chooseFile")}
                  </Button>
                </div>
                <textarea
                  {...TECHNICAL_INPUT_PROPS}
                  required={isCreate}
                  value={auth.keyContent}
                  onChange={(e) => setAuth({ ...auth, keyContent: e.target.value })}
                  rows={4}
                  className="mt-1 w-full rounded border border-border-strong bg-surface px-2 py-1.5 font-mono text-xs text-fg"
                />
                {/* Spec 0069, Teil C2 (BL-0110): reiner Hinweistext — keine
                 * Prüfung des Inhalts, keine Sperre, kein Einfluss auf
                 * Speichern/Verbinden. RSA-Schlüssel funktionieren
                 * unverändert weiter. */}
                <p className="mt-1 text-xs text-fg-subtle">{t("serverForm.ed25519Recommendation")}</p>
              </div>
              <Field label={t("serverForm.passphraseOptional")}>
                {(control) => (
                  <input
                    {...control}
                    {...TECHNICAL_INPUT_PROPS}
                    type="password"
                    value={auth.passphrase}
                    onChange={(e) => setAuth({ ...auth, passphrase: e.target.value })}
                    className={`${FIELD_CONTROL_CLASS} w-full`}
                  />
                )}
              </Field>
            </div>
          )}

          {auth.kind === "agent" && (
            <p className="text-sm text-fg-subtle">{t("serverForm.agentHint")}</p>
          )}

          {auth.kind === "certificate" && (
            <div className="space-y-2">
              <div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="text-sm text-fg-muted">
                    {t("serverForm.certificateLabel")}{" "}
                    {!isCreate && <span className="text-fg-subtle">{t("serverForm.unchangedHint")}</span>}
                  </span>
                  <Button size="sm"
                    onClick={async () => {
                      const content = await pickAndReadTextFile(
                        t("serverForm.chooseCertificateDialogTitle"),
                      );
                      if (content !== null) setAuth({ ...auth, certContent: content });
                    }}
                  >
                    {t("serverForm.chooseFile")}
                  </Button>
                </div>
                <textarea
                  {...TECHNICAL_INPUT_PROPS}
                  required={isCreate}
                  value={auth.certContent}
                  onChange={(e) => setAuth({ ...auth, certContent: e.target.value })}
                  rows={3}
                  className="mt-1 w-full rounded border border-border-strong bg-surface px-2 py-1.5 font-mono text-xs text-fg"
                />
              </div>
              <div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="text-sm text-fg-muted">
                    {t("serverForm.associatedKey")}{" "}
                    {!isCreate && <span className="text-fg-subtle">{t("serverForm.unchangedHint")}</span>}
                  </span>
                  <Button size="sm"
                    onClick={async () => {
                      const content = await pickAndReadTextFile(t("serverForm.chooseKeyDialogTitle"));
                      if (content !== null) setAuth({ ...auth, keyContent: content });
                    }}
                  >
                    {t("serverForm.chooseFile")}
                  </Button>
                </div>
                <textarea
                  {...TECHNICAL_INPUT_PROPS}
                  required={isCreate}
                  value={auth.keyContent}
                  onChange={(e) => setAuth({ ...auth, keyContent: e.target.value })}
                  rows={3}
                  className="mt-1 w-full rounded border border-border-strong bg-surface px-2 py-1.5 font-mono text-xs text-fg"
                />
              </div>
            </div>
          )}

          {auth.kind === "identityFile" && (
            <div className="space-y-2">
              <div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="text-sm text-fg-muted">{t("serverForm.identityFilePathLabel")}</span>
                  <Button size="sm"
                    onClick={async () => {
                      const path = await pickFilePath(t("serverForm.chooseIdentityFileDialogTitle"));
                      if (path !== null) setAuth({ ...auth, path });
                    }}
                  >
                    {t("serverForm.chooseFile")}
                  </Button>
                </div>
                <input
                  {...TECHNICAL_INPUT_PROPS}
                  type="text"
                  required
                  value={auth.path}
                  onChange={(e) => setAuth({ ...auth, path: e.target.value })}
                  placeholder={t("serverForm.identityFilePathPlaceholder")}
                  className="mt-1 w-full rounded border border-border-strong bg-surface px-2 py-1.5 font-mono text-xs text-fg"
                />
                {/* Spec 0076, B-3: Vorab-Befund — hindert das Speichern
                 * nicht, die Datei darf erst später entstehen. */}
                <IdentityFileFacts
                  loading={identityFactsLoading}
                  facts={identityFacts}
                  error={identityFactsError}
                  emptyPath={auth.path.trim() === ""}
                  path={auth.path}
                />
              </div>
              <Field label={t("serverForm.passphraseOptional")}>
                {(control) => (
                  <input
                    {...control}
                    {...TECHNICAL_INPUT_PROPS}
                    type="password"
                    value={auth.passphrase}
                    onChange={(e) => setAuth({ ...auth, passphrase: e.target.value })}
                    className={`${FIELD_CONTROL_CLASS} w-full`}
                  />
                )}
              </Field>
            </div>
          )}
        </fieldset>

        {/* Issue #667: shown once when a key-file server is saved with a new
         * or changed path. */}
        {importPromptOpen && (
          <DialogFrame
            layer="app"
            size="md"
            title={t("serverForm.importKey.title")}
            onClose={cancelImportPrompt}
            initialFocusRef={importPrimaryRef}
            actions={
              <>
                <Button size="sm" disabled={saving} onClick={cancelImportPrompt}>
                  {t("common.cancel")}
                </Button>
                {platformCapabilities(detectFallbackPlatform()).keyFilePath && (
                  <Button
                    size="sm"
                    disabled={saving}
                    onClick={() => (importSavedId !== null ? finishKeepingPath() : save(false))}
                  >
                    {t("serverForm.importKey.keep")}
                  </Button>
                )}
                <Button ref={importPrimaryRef} variant="primary" size="sm" disabled={saving} onClick={() => save(true)}>
                  {t("serverForm.importKey.import")}
                </Button>
              </>
            }
          >
            <p className="mb-2 font-mono text-xs break-all text-fg-muted">{auth.kind === "identityFile" ? auth.path : ""}</p>
            <ul className="mb-3 space-y-1 text-sm text-fg-muted">
              <li>{t("serverForm.importKey.importHint")}</li>
              {platformCapabilities(detectFallbackPlatform()).keyFilePath && (
                <li>{t("serverForm.importKey.keepHint")}</li>
              )}
            </ul>
            {importError && (
              <p role="alert" className="mb-2 text-xs text-danger-fg">
                {importError}
              </p>
            )}
          </DialogFrame>
        )}

        {/* Spec 0076, C-1/C-2/C-7 (BL-0222): nur für einen bereits
         * gespeicherten Server, dessen Anmeldeart tatsächlich (nicht nur im
         * Entwurf) eine Schlüsseldatei ist — der Knopf wirkt auf den
         * gespeicherten Server, nicht auf den gerade offenen Entwurf. */}
        {!isCreate && loaded && loaded.authKind === "identity_file" && (
          <fieldset className="rounded border border-border p-3">
            <legend className="px-1 text-sm text-fg-muted">
              {t("serverForm.convertToKeychain.legend")}
            </legend>
            <p className="mb-2 text-xs text-fg-subtle">{t("serverForm.convertToKeychain.hint")}</p>
            {!convertConfirmOpen && (
              <>
                  <Button size="sm"
                    onClick={() => setConvertConfirmOpen(true)}
                    disabled={!convertFacts?.validKey}
                  >
                    {t("serverForm.convertToKeychain.button")}
                  </Button>
                  {!convertFacts?.validKey && (
                    <p className="mt-2 text-xs text-warning-fg">
                      {convertFactsError
                        ? t("serverForm.convertToKeychain.checkFailed")
                        : convertFacts
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
              <div className="rounded border border-border-strong bg-surface-sunken p-3 text-sm">
                <p className="mb-2 text-fg-soft">{t("serverForm.convertToKeychain.confirmWhatChanges")}</p>
                <p className="mb-2 text-fg-subtle">{t("serverForm.convertToKeychain.confirmWhatStays")}</p>
                <p className="mb-3 font-mono text-xs break-all text-fg-muted">
                  {loaded.identityFilePath}
                </p>
                {convertError && <p className="mb-2 text-xs text-danger-fg">{convertError}</p>}
                <div className="flex gap-2">
                  <Button size="sm"
                    onClick={() => {
                      setConvertConfirmOpen(false);
                      setConvertError(null);
                    }}
                  >
                    {t("common.cancel")}
                  </Button>
                  <Button variant="primary" size="sm"
                    onClick={handleConvertToKeychain}
                    disabled={converting}
                  >
                    {converting
                      ? t("serverForm.convertToKeychain.converting")
                      : t("serverForm.convertToKeychain.confirm")}
                  </Button>
                </div>
              </div>
            )}
          </fieldset>
        )}

        <fieldset className="rounded border border-border p-3">
          <legend className="px-1 text-sm text-fg-muted">{t("serverForm.sudoFieldset")}</legend>
          <p className="mb-2 text-xs text-fg-subtle">{t("serverForm.sudoHint")}</p>
          <Field
            label={
              <>
                {t("serverForm.sudoLabel")}{" "}
                <span className="text-fg-subtle">
                  {sudoPasswordUnknown
                    ? t("serverForm.sudoUnchangedUnknown")
                    : hasSudoPassword
                      ? t("serverForm.sudoUnchangedStored")
                      : t("serverForm.sudoUnchanged")}
                </span>
              </>
            }
          >
            {(control) => (
              <input
                {...control}
                {...TECHNICAL_INPUT_PROPS}
                type="password"
                value={sudoPassword}
                onChange={(e) => setSudoPassword(e.target.value)}
                className={`${FIELD_CONTROL_CLASS} w-full`}
              />
            )}
          </Field>
          {(hasSudoPassword || sudoPasswordUnknown) && (
            <Button size="sm"
              onClick={handleClearSudoPassword}
              disabled={clearingSudoPassword}
              className="mt-2"
            >
              {clearingSudoPassword ? t("common.removing") : t("serverForm.removeSudoPassword")}
            </Button>
          )}
        </fieldset>

        {!isLocal && (
          <Field
            label={t("serverForm.startDirectoryLabel")}
            help={startDirectoryError ? undefined : t("serverForm.startDirectoryHint")}
            error={startDirectoryError ?? undefined}
          >
            {(control) => (
              <input
                {...control}
                {...TECHNICAL_INPUT_PROPS}
                value={startDirectory}
                onChange={(e) => setStartDirectory(e.target.value)}
                placeholder={t("serverForm.startDirectoryPlaceholder")}
                className={`${FIELD_CONTROL_CLASS} w-full font-mono`}
              />
            )}
          </Field>
        )}

        {!isLocal && (
          <details className="rounded border border-border p-3">
            <summary className="cursor-pointer text-sm text-fg-muted">
              {t("serverForm.advanced")}
            </summary>
            <Field
              label={t("serverForm.sftpServerPathLabel")}
              help={t("serverForm.sftpServerPathHint")}
              className="mt-3"
            >
              {(control) => (
                <input
                  {...control}
                  {...TECHNICAL_INPUT_PROPS}
                  value={sftpServerPath}
                  onChange={(e) => setSftpServerPath(e.target.value)}
                  placeholder={t("serverForm.sftpServerPathPlaceholder")}
                  className={`${FIELD_CONTROL_CLASS} w-full font-mono`}
                />
              )}
            </Field>
          </details>
        )}


        <div className="flex flex-wrap items-center gap-2">
          <Button onClick={runTest} disabled={testing}>
            {testing ? t("common.testing") : t("serverForm.testConnection")}
          </Button>
          {testResult && <TestResultBadge result={testResult} />}
        </div>
        {/* Issue #51: Schritt-Protokoll des letzten Tests, zugeklappt —
         * bei Erfolg wie bei Fehlschlag. */}
        {testResult?.steps && testResult.steps.length > 0 && (
          <ConnectStepLog steps={testResult.steps} />
        )}
        </TabPanel>

        <TabPanel idBase={tabsIdBase} id="security" active={activeTab === "security"} className="space-y-3">
        <fieldset className="rounded border border-border p-3">
          <legend className="px-1 text-sm text-fg-muted">
            {t("serverForm.postIngestFieldset")}
          </legend>
          <p className="mb-2 text-xs text-fg-subtle">{t("serverForm.postIngestHint")}</p>
          <div className="space-y-2">
            {(["strict", "balanced", "standard"] as PostIngestPolicy[]).map((option) => (
              <label key={option} className="flex items-start gap-2 text-sm text-fg-muted">
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
                  <span className="text-xs text-fg-subtle">
                    {t(`serverForm.postIngest.${option}.description`)}
                  </span>
                </span>
              </label>
            ))}
          </div>
        </fieldset>

        <fieldset className="rounded border border-border p-3">
          <legend className="px-1 text-sm text-fg-muted">
            {t("serverForm.aiInjectionCheckFieldset")}
          </legend>
          <label className="flex items-start gap-2 text-sm text-fg-muted">
            <input
              type="checkbox"
              checked={aiInjectionCheckEnabled}
              disabled={!secondOpinionAvailable}
              onChange={(e) => setAiInjectionCheckEnabled(e.target.checked)}
              className="mt-1"
            />
            <span>{t("serverForm.aiInjectionCheckLabel")}</span>
          </label>
          <p className="mt-2 text-xs text-fg-subtle">
            {secondOpinionAvailable
              ? t("serverForm.aiInjectionCheckHint")
              : t("serverForm.aiInjectionCheckUnavailableHint")}
          </p>
        </fieldset>

        {!isCreate && !isLocal && storedHostKeys !== null && (
          <section aria-labelledby="stored-host-keys-heading" className="space-y-1">
            <h3 id="stored-host-keys-heading" className="text-sm font-medium text-fg-muted">
              {t("serverForm.hostKeys.title")}
            </h3>
            {storedHostKeys.length === 0 ? (
              <p className="text-xs text-fg-subtle">{t("serverForm.hostKeys.none")}</p>
            ) : (
              <ul className="space-y-1">
                {storedHostKeys.map((k) => (
                  <li key={`${k.algorithm}:${k.fingerprint}`} className="text-xs text-fg-muted">
                    <span className="font-mono">{k.algorithm}</span>{" "}
                    <span className="select-text break-all font-mono">{k.fingerprint}</span>
                  </li>
                ))}
              </ul>
            )}
          </section>
        )}
        </TabPanel>

        {error && <p className="text-sm text-danger-fg">{error}</p>}

        {activeTab !== "notes" && (
          <div className="narrow:sticky narrow:bottom-0 narrow:z-10 narrow:-mx-4 narrow:border-t narrow:border-surface-raised narrow:bg-surface narrow:px-4 narrow:py-2">
            <div className="flex flex-wrap items-center gap-2 narrow:flex-col narrow:items-stretch">
              <Button variant="primary" type="submit" disabled={saving} className="narrow:min-h-11 narrow:w-full">
                {saving ? t("common.saving") : isCreate ? t("common.create") : t("common.save")}
              </Button>
              {!isCreate && serverId && loaded && onConnect && (
                <Button
                  type="button"
                  disabled={dirty || saving}
                  title={dirty ? t("serverForm.connectSaveFirst") : undefined}
                  onClick={() => onConnect(serverId)}
                  className="narrow:min-h-11 narrow:w-full"
                >
                  {t("serverForm.connect")}
                </Button>
              )}
              {!isCreate && dirty && onConnect && (
                <span className="text-xs text-fg-subtle" data-testid="connect-save-first-hint">
                  {t("serverForm.connectSaveFirst")}
                </span>
              )}
            </div>
          </div>
        )}
      </form>

      {!isCreate && serverId && loaded && (
        <>
          <ServerFormTabSections
            serverId={serverId}
            dirty={dirty}
            tab="connection"
            hidden={activeTab !== "connection"}
            labelledBy={tabId(tabsIdBase, "connection")}
          />
          <ServerFormTabSections
            serverId={serverId}
            dirty={dirty}
            tab="security"
            hidden={activeTab !== "security"}
            labelledBy={tabId(tabsIdBase, "security")}
          />
          <div hidden={activeTab !== "general"} className="space-y-6">
            <ServerFormSections serverId={serverId} dirty={dirty} tab="general" />

            <div className="border-t border-border pt-4">
              <Button variant="danger" size="sm"
                onClick={handleDeleteClick}
              >
                {t("serverForm.deleteServer")}
              </Button>

              {/* Spec 0071, A17: Der Server ist weg, aber mindestens ein
               * Secret blieb im Schlüsselbund. Kein stilles Schließen — der
               * Nutzer braucht die Refs, um die verwaisten Einträge im
               * Schlüsselbund-Verwaltungsprogramm wiederzufinden. */}
              {secretsLeftBehind && (
                <div
                  className="mt-3 rounded border border-warning-border-strong bg-warning-subtle p-3 text-sm"
                  data-testid="secrets-left-behind"
                >
                  <p className="mb-2 font-medium text-warning-fg-softer">
                    {t("serverForm.deletedWithLeftoverSecretsTitle")}
                  </p>
                  <p className="mb-2 text-warning-fg-softer">
                    {t("serverForm.deletedWithLeftoverSecretsHint")}
                  </p>
                  <ul className="mb-2 space-y-1 font-mono text-xs text-warning-fg-softer">
                    {secretsLeftBehind.map((ref) => (
                      <li key={ref}>{ref}</li>
                    ))}
                  </ul>
                  <Button size="sm"
                    onClick={() => {
                      setSecretsLeftBehind(null);
                      onDeleted();
                    }}
                  >
                    {t("common.close")}
                  </Button>
                </div>
              )}

              {deletePreview && (
                <div className="mt-3 rounded border border-danger-border bg-danger-subtle p-3 text-sm">
                  <p className="mb-2 font-medium text-danger-fg-softer">{t("serverForm.deleteImpactTitle")}</p>
                  {(deletePreview.server.authKind === "agent" ||
                    deletePreview.server.authKind === "no_password") &&
                  !deletePreview.server.hasSudoPassword &&
                  !deletePreview.server.sudoPasswordUnknown &&
                  deletePreview.serversLosingJumpHost.length === 0 ? (
                    <p className="mb-2 text-danger-fg-softer">{t("serverForm.deleteNoKeychainImpact")}</p>
                  ) : (
                    <ul className="mb-2 space-y-1 text-danger-fg-softer">
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
                        deletePreview.server.authKind !== "no_password" &&
                        deletePreview.server.authKind !== "identity_file" && (
                          <li>
                            {t("serverForm.secretWillBeDeleted", {
                              label: authKindLabels(t)[deletePreview.server.authKind],
                            })}
                          </li>
                        )}
                      {deletePreview.server.authKind === "identity_file" && (
                        <li>{t("serverForm.identityFilePassphraseMayBeDeleted")}</li>
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
                  <p className="mb-2 text-danger-fg-softer">{t("serverForm.deleteAlwaysRemovesHistory")}</p>
                  <div className="flex gap-2">
                    <Button size="sm"
                      onClick={() => setDeletePreview(null)}
                    >
                      {t("common.cancel")}
                    </Button>
                    <Button variant="danger" size="sm"
                      onClick={handleConfirmDelete}
                      disabled={deleting}
                    >
                      {deleting ? t("common.deleting") : t("serverForm.confirmDeleteServer")}
                    </Button>
                  </div>
                </div>
              )}
            </div>
          </div>
          <TabPanel idBase={tabsIdBase} id="notes" active={activeTab === "notes"} className="space-y-6">
            <NotesPanel
              target={{ Server: serverId }}
              currentNotes={loaded.notes}
              onNotesChanged={onSaved}
              autoFocus={autoFocusNotes}
            />

            <div>
              <Button size="sm"
                onClick={handlePreview}
                disabled={previewLoading}
              >
                {previewLoading ? t("common.loading") : t("serverForm.contextPreview")}
              </Button>
              {preview !== null && (
                <pre className="mt-2 max-h-64 overflow-y-auto whitespace-pre-wrap rounded border border-border bg-surface-sunken p-3 text-xs text-fg-muted">
                  {preview || t("serverForm.noContext")}
                </pre>
              )}
            </div>
          <ServerFormSections serverId={serverId} dirty={dirty} tab="notes" />
          </TabPanel>
          {renderExtensionPanels(serverId)}
        </>
      )}

      {testDialog && (
        <TestConnectionDialog
          steps={testDialog.steps}
          result={testing ? null : testResult}
          onCancel={cancelTest}
          onClose={closeTestDialog}
        />
      )}

      {pendingHostKey && !testDialog && (
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
