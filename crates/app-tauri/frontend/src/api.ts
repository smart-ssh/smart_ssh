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
  RuleDto,
  RuleInput,
  Scope,
  ServerDto,
  ServerInput,
  TestConnectionResult,
} from "./types";

/** Von `crate::error::CommandError` (`crates/app-tauri/src/error.rs`). */
export interface CommandErrorPayload {
  message: string;
}

/** Extrahiert eine anzeigbare Meldung aus einem abgelehnten `invoke()`. */
export function commandErrorMessage(err: unknown): string {
  if (typeof err === "object" && err !== null && "message" in err) {
    const message = (err as CommandErrorPayload).message;
    if (typeof message === "string") return message;
  }
  return String(err);
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
