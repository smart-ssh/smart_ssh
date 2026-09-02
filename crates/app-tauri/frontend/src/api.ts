import { invoke } from "@tauri-apps/api/core";
import type {
  ActionUserDecision,
  AiProviderConfigDto,
  AiProviderConfigInput,
  DeleteGroupResult,
  DocumentFormat,
  EvalContextInput,
  EvaluationTraceDto,
  GroupDto,
  HostKeyUserDecision,
  NoteRevisionDto,
  NoteTarget,
  PatternDto,
  PatternSuggestionDto,
  PatternType,
  RemoteEntryDto,
  RuleDto,
  RuleInput,
  Scope,
  ServerDto,
  ServerInput,
  SessionSummaryDto,
  TestConnectionResult,
} from "./types";

/** Von `crate::error::CommandError` (`crates/app-tauri/src/error.rs`). */
export interface CommandErrorPayload {
  message: string;
  /** Spec 0024, Abschnitt 5: stabiler Code fürs Frontend-Mapping (s.
   * `errorCodes.ts`) — `null`/fehlend bei den meisten Fehlern, gesetzt nur
   * für die Validierungsfehler aus den Server-/Gruppen-Formularen. */
  code?: string | null;
}

/** Extrahiert eine anzeigbare Meldung aus einem abgelehnten `invoke()`. */
export function commandErrorMessage(err: unknown): string {
  if (typeof err === "object" && err !== null && "message" in err) {
    const message = (err as CommandErrorPayload).message;
    if (typeof message === "string") return message;
  }
  return String(err);
}

/** Extrahiert den stabilen `code` aus einem abgelehnten `invoke()`, falls
 * vorhanden (s. `commandErrorMessage`-Gegenstück). */
export function commandErrorCode(err: unknown): string | null {
  if (typeof err === "object" && err !== null && "code" in err) {
    const code = (err as CommandErrorPayload).code;
    if (typeof code === "string") return code;
  }
  return null;
}

export const listServers = (groupId?: string) =>
  invoke<ServerDto[]>("list_servers", { groupId: groupId ?? null });

export const listAiProviders = () => invoke<AiProviderConfigDto[]>("list_ai_providers");

export const addAiProvider = (config: AiProviderConfigInput) =>
  invoke<string>("add_ai_provider", { config });

export const updateAiProvider = (id: string, config: AiProviderConfigInput) =>
  invoke<void>("update_ai_provider", { id, config });

export const deleteAiProvider = (id: string) => invoke<void>("delete_ai_provider", { id });

export const setActiveAiProvider = (id: string) =>
  invoke<void>("set_active_ai_provider", { id });

// --- Teil 2: Session/Terminal/Chat --------------------------------------

export const connect = (serverId: string) => invoke<string>("connect", { serverId });

export const confirmHostKey = (sessionId: string, decision: HostKeyUserDecision) =>
  invoke<void>("confirm_host_key", { sessionId, decision });

export const openTerminal = (sessionId: string) => invoke<void>("open_terminal", { sessionId });

export const terminalInput = (sessionId: string, data: Uint8Array) =>
  invoke<void>("terminal_input", { sessionId, data: Array.from(data) });

export const terminalResize = (sessionId: string, cols: number, rows: number) =>
  invoke<void>("terminal_resize", { sessionId, cols, rows });

export const sendChatMessage = (sessionId: string, text: string) =>
  invoke<void>("send_chat_message", { sessionId, text });

export const respondToAction = (
  sessionId: string,
  actionId: string,
  decision: ActionUserDecision,
) => invoke<void>("respond_to_action", { sessionId, actionId, decision });

export const disconnect = (sessionId: string) => invoke<void>("disconnect", { sessionId });

/** Spec 0021, Abschnitt 5: bricht die automatische Fortsetzungskette für die
 * aktuelle Nutzer-Nachricht sofort ab — lässt einen bereits offenen
 * Bestätigungsdialog unangetastet (s. `crate::commands::
 * stop_auto_continuation`-Doc-Kommentar). */
export const stopAutoContinuation = (sessionId: string) =>
  invoke<void>("stop_auto_continuation", { sessionId });

// --- Spec 0017: Multi-Tab-Sessions --------------------------------------

/** Maßgebliche Quelle dafür, welche Sessions tatsächlich offen sind — dient
 * dem Wiederherstellen der Tab-Leiste bei einem Frontend-Neuladen (s.
 * `crate::commands::list_sessions`). */
export const listSessions = () => invoke<SessionSummaryDto[]>("list_sessions");

// --- Spec 0008: Server-/Gruppen-Verwaltung ------------------------------

export const listGroups = () => invoke<GroupDto[]>("list_groups");

export const createGroup = (name: string, parentId: string | null) =>
  invoke<string>("create_group", { name, parentId });

export const updateGroup = (id: string, name: string, parentId: string | null) =>
  invoke<void>("update_group", { id, name, parentId });

export const deleteGroup = (id: string, confirmCascade: boolean) =>
  invoke<DeleteGroupResult>("delete_group", { id, confirmCascade });

export const getServer = (id: string) => invoke<ServerDto>("get_server", { id });

export const createServer = (input: ServerInput) =>
  invoke<string>("create_server", { input });

export const updateServer = (id: string, input: ServerInput) =>
  invoke<void>("update_server", { id, input });

export const deleteServer = (id: string) => invoke<void>("delete_server", { id });

/** Spec 0018, Abschnitt 4: expliziter Entfernen-Weg — ein leeres
 * `sudoPassword`-Feld in `updateServer` bedeutet bereits "unverändert". */
export const clearServerSudoPassword = (id: string) =>
  invoke<void>("clear_server_sudo_password", { id });

export const testConnection = (input: ServerInput, existingServerId?: string) =>
  invoke<TestConnectionResult>("test_connection", {
    input,
    existingServerId: existingServerId ?? null,
  });

export const trustHostKey = (host: string, port: number, rawKey: number[]) =>
  invoke<void>("trust_host_key", { host, port, rawKey });

export const updateGroupNotes = (id: string, content: string) =>
  invoke<void>("update_group_notes", { id, content });

export const updateServerNotes = (id: string, content: string) =>
  invoke<void>("update_server_notes", { id, content });

export const listNoteRevisions = (target: NoteTarget) =>
  invoke<NoteRevisionDto[]>("list_note_revisions", { target });

export const rollbackNote = (target: NoteTarget, revisionId: string) =>
  invoke<void>("rollback_note", { target, revisionId });

export const previewEffectiveNotes = (serverId: string) =>
  invoke<string>("preview_effective_notes", { serverId });

// --- Spec 0009: Filter-Regel-Verwaltung ---------------------------------

export const listRules = (scopeFilter?: Scope) =>
  invoke<RuleDto[]>("list_rules", { scopeFilter: scopeFilter ?? null });

export const createRule = (input: RuleInput) => invoke<string>("create_rule", { input });

export const updateRule = (id: string, input: RuleInput) =>
  invoke<void>("update_rule", { id, input });

export const deleteRule = (id: string) => invoke<void>("delete_rule", { id });

export const listHardBlacklist = () => invoke<PatternDto[]>("list_hard_blacklist");

export const listKnownTags = () => invoke<string[]>("list_known_tags");

export const evaluateExplained = (command: string, ctx: EvalContextInput) =>
  invoke<EvaluationTraceDto>("evaluate_explained", { command, ctx });

// --- Spec 0011: Regel-Schnellvorschlag im Bestätigungsdialog ------------

export const suggestRulePatterns = (command: string) =>
  invoke<PatternSuggestionDto[]>("suggest_rule_patterns", { command });

export const acceptAndCreateRule = (
  sessionId: string,
  actionId: string,
  patternType: PatternType,
  patternValue: string,
  scope: Scope,
  priority?: number,
) =>
  invoke<string>("accept_and_create_rule", {
    sessionId,
    actionId,
    patternType,
    patternValue,
    scope,
    priority: priority ?? null,
  });

// --- Spec 0012: KI-generierte Dokumente ---------------------------------

export const exportDocument = (contentMarkdown: string, title: string, format: DocumentFormat) =>
  invoke<void>("export_document", { contentMarkdown, title, format });

// --- Spec 0015: Chat-Prompt-Historie -------------------------------------

/** Chronologisch aufsteigend (älteste zuerst), s. `crate::commands::list_prompt_history`. */
export const listPromptHistory = (serverId: string) =>
  invoke<string[]>("list_prompt_history", { serverId });

// --- Spec 0016: Strukturiertes Logging & Diagnose -----------------------

/** Öffnet den Log-Ordner im System-Dateimanager (Finder/Explorer). */
export const openLogDirectory = () => invoke<void>("open_log_directory");

// --- Spec 0020, Abschnitt 5: Manueller Dateibrowser ---------------------

export const sftpList = (sessionId: string, path: string) =>
  invoke<RemoteEntryDto[]>("sftp_list", { sessionId, path });

/** Öffnet den nativen Speichern-Dialog im Backend — kehrt ohne Fehler
 * zurück, wenn der Nutzer abbricht (s. `crate::commands::sftp_download`). */
export const sftpDownload = (sessionId: string, remotePath: string) =>
  invoke<void>("sftp_download", { sessionId, remotePath });

/** `localPath` muss bereits aufgelöst sein (nativer Öffnen-Dialog oder
 * OS-Drag-and-Drop, s. `crate::commands::sftp_upload`-Doc-Kommentar). */
export const sftpUpload = (sessionId: string, localPath: string, remotePath: string) =>
  invoke<void>("sftp_upload", { sessionId, localPath, remotePath });

export const sftpDelete = (sessionId: string, path: string) =>
  invoke<void>("sftp_delete", { sessionId, path });

export const sftpRename = (sessionId: string, from: string, to: string) =>
  invoke<void>("sftp_rename", { sessionId, from, to });

export const sftpMkdir = (sessionId: string, path: string) =>
  invoke<void>("sftp_mkdir", { sessionId, path });
