import type {
  AuthMethodInput,
  AuthMethodKind,
  PostIngestPolicy,
  ServerDto,
} from "../../types";

/** Formular-lokaler Zustand — getrennt von [`AuthMethodInput`], weil
 * Textfelder immer einen String brauchen (nie `null`), auch wenn die
 * Bedeutung "leer = unverändert" (Update) erst beim Absenden entsteht
 * (s. `toAuthMethodInput`). */
export type AuthFormState =
  // `saved`: der Server hat ein gespeichertes Passwort (Issue #632) — nur
  // dann bedeutet ein leeres Feld "unverändert" und gibt es "entfernen".
  | { kind: "password"; value: string; saved: boolean; remove: boolean }
  | { kind: "privateKey"; keyContent: string; passphrase: string }
  | { kind: "agent" }
  | { kind: "certificate"; certContent: string; keyContent: string }
  | { kind: "identityFile"; path: string; passphrase: string };

export function authStateFromKind(kind: AuthFormState["kind"]): AuthFormState {
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
export function toAuthMethodInput(state: AuthFormState, isCreate: boolean): AuthMethodInput {
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


/** All field values of the server form plus the loaded snapshot and the
 * dirty flag. Secrets (sudo password, passphrases) live only in the draft
 * fields; `loaded` is the backend DTO, which never carries them. */
export interface ServerFormState {
  /** Backend snapshot of the saved server; `null` while creating/loading. */
  loaded: ServerDto | null;
  /** Unsaved changes (Spec 0038, section 4). Set by `markDirty` (form
   * `onChange`), cleared by `load`/`saved`; editing a value back does not
   * clear it. */
  dirty: boolean;
  name: string;
  host: string;
  port: string;
  username: string;
  groupId: string | null;
  jumpHost: string | null;
  tags: string[];
  tagDraft: string;
  auth: AuthFormState;
  sudoPassword: string;
  hasSudoPassword: boolean;
  sudoPasswordUnknown: boolean;
  postIngestPolicy: PostIngestPolicy;
  aiInjectionCheckEnabled: boolean;
  sftpServerPath: string;
  startDirectory: string;
  localNotes: string;
}

/** Fields that `setField` can write directly. */
export type ServerFormFieldKey = Exclude<keyof ServerFormState, "loaded" | "dirty" | "tags" | "auth">;

export type ServerFormAction =
  | { type: "load"; server: ServerDto }
  | { type: "setField"; field: ServerFormFieldKey; value: ServerFormState[ServerFormFieldKey] }
  | { type: "setAuth"; auth: AuthFormState }
  | { type: "addTag" }
  | { type: "removeTag"; tag: string }
  | { type: "markDirty" }
  | { type: "reset"; defaultGroupId: string | null }
  | { type: "saved" };

export function initialServerFormState(defaultGroupId: string | null): ServerFormState {
  return {
    loaded: null,
    dirty: false,
    name: "",
    host: "",
    port: "22",
    username: "",
    groupId: defaultGroupId,
    jumpHost: null,
    tags: [],
    tagDraft: "",
    auth: authStateFromKind("password"),
    sudoPassword: "",
    hasSudoPassword: false,
    sudoPasswordUnknown: false,
    postIngestPolicy: "balanced",
    aiInjectionCheckEnabled: false,
    sftpServerPath: "",
    startDirectory: "",
    localNotes: "",
  };
}

function authStateFromServer(server: ServerDto): AuthFormState {
  // Spec 0076, B-4: der Pfad kommt vorbefüllt aus dem DTO — anders
  // als bei den übrigen Anmeldearten reicht `authStateFromKind`
  // allein nicht, das leere Formularfeld bräuchte sonst erst einen
  // manuellen Klick auf "Datei wählen", obwohl der Server längst
  // einen Pfad hat.
  if (server.authKind === "identity_file") {
    return { kind: "identityFile", path: server.identityFilePath ?? "", passphrase: "" };
  }
  if (server.authKind === "password" || server.authKind === "no_password") {
    return { kind: "password", value: "", saved: server.authKind === "password", remove: false };
  }
  return authStateFromKind(server.authKind === "private_key" ? "privateKey" : server.authKind);
}

export function serverFormReducer(state: ServerFormState, action: ServerFormAction): ServerFormState {
  switch (action.type) {
    case "load": {
      const server = action.server;
      return {
        ...state,
        loaded: server,
        dirty: false,
        name: server.name,
        host: server.host,
        port: String(server.port),
        username: server.username,
        groupId: server.groupId,
        jumpHost: server.jumpHost,
        tags: server.tags,
        auth: authStateFromServer(server),
        sudoPassword: "",
        hasSudoPassword: server.hasSudoPassword,
        sudoPasswordUnknown: server.sudoPasswordUnknown,
        localNotes: server.notes,
        postIngestPolicy: server.postIngestPolicy,
        aiInjectionCheckEnabled: server.aiInjectionCheckEnabled,
        sftpServerPath: server.sftpServerPath ?? "",
        startDirectory: server.startDirectory ?? "",
      };
    }
    case "setField":
      return { ...state, [action.field]: action.value };
    case "setAuth":
      return { ...state, auth: action.auth };
    case "addTag": {
      const value = state.tagDraft.trim();
      if (value && !state.tags.includes(value)) {
        return { ...state, tags: [...state.tags, value], tagDraft: "" };
      }
      return { ...state, tagDraft: "" };
    }
    case "removeTag":
      return { ...state, tags: state.tags.filter((tagValue) => tagValue !== action.tag) };
    case "markDirty":
      return state.dirty ? state : { ...state, dirty: true };
    case "reset":
      // Create mode: back to empty defaults. Like before, `dirty`, the
      // tag draft, local notes, start directory and SFTP path stay as they are.
      return {
        ...state,
        loaded: null,
        name: "",
        host: "",
        port: "22",
        username: "",
        groupId: action.defaultGroupId,
        jumpHost: null,
        tags: [],
        auth: authStateFromKind("password"),
        sudoPassword: "",
        hasSudoPassword: false,
        postIngestPolicy: "balanced",
        aiInjectionCheckEnabled: false,
      };
    case "saved":
      return state.dirty ? { ...state, dirty: false } : state;
  }
}

/** `t` wird als Parameter durchgereicht statt selbst `useTranslation()`
 * aufzurufen — diese Funktion läuft außerhalb einer Komponente, Hooks sind
 * hier nicht erlaubt. */
export function authKindLabels(
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
