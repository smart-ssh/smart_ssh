# Code map

Where things live in smart_ssh. Every file and symbol named here is checked by
`npm run check-codemap` (run by `npm run lint`), so the map cannot silently go
stale.

**Notation.** A backticked token containing a slash is a repo path and must exist.
Backticked identifiers (`name`, `Type::name`, `name()`) that follow a path in
the same table row or list item are symbols of that path; the file (for a
directory: any Rust/TS file below it) must contain a definition
(`fn|struct|enum|trait|type|const|static|mod|macro_rules!` in Rust,
`function|const|let|class|interface|type|enum` in TS/TSX). Paths and symbols in
fenced code blocks are not checked. When you rename or move something named
here, update this file in the same change.

## 1. Layers

| Crate | Responsibility | Most important items |
|---|---|---|
| `crates/core/` | Pure domain logic, no Tauri and no UI framework; all external boundaries are traits | `crates/core/src/ssh/transport.rs` — `SshTransport`, `SftpSession`, `InteractiveShell`; `crates/core/src/ai/provider.rs` — `AiProvider`; `crates/core/src/profiles/store.rs` — `ProfileStore`; `crates/core/src/profiles/credentials.rs` — `CredentialStore`; `crates/core/src/profiles/types.rs` — `Server`, `AiAction`; `crates/core/src/entitlements.rs` — `Feature` |
| `crates/core/src/filter/` | Policy engine (security-critical) | `crates/core/src/filter/engine.rs` — `FilterEngine`, `PolicySource`; `crates/core/src/filter/parser.rs` — `split_top_level_segments`; `crates/core/src/filter/types.rs` — `Rule`, `EvalContext` |
| `crates/core/src/risk/` | Rule-based risk classification (security-critical) | `crates/core/src/risk/classifier.rs` — `RuleBasedRiskClassifier`, `CommandCheckFinding`, `secret_path_read_reason` |
| `crates/core/src/ai/` | AI event types, output redaction, fencing of untrusted content | `crates/core/src/ai/types.rs` — `AiEvent`, `SessionContext`; `crates/core/src/ai/redactor.rs` — `OutputRedactor`, `DefaultOutputRedactor`; `crates/core/src/ai/fencing.rs` — `fence_untrusted`, `UntrustedKind`; `crates/core/src/ai/second_opinion.rs` |
| `crates/ai-providers/` | Concrete AI provider implementations | `crates/ai-providers/src/anthropic.rs` — `AnthropicProvider`; `crates/ai-providers/src/openai_compatible.rs` — `OpenAiCompatibleProvider`; `crates/ai-providers/src/openai_responses.rs` — `OpenAiResponsesProvider`; `crates/ai-providers/src/retry.rs` (429 retry); `crates/ai-providers/src/rate_limit_budget.rs` |
| `crates/ssh-transport/` | Concrete transports (russh and the local pseudo-server) | `crates/ssh-transport/src/transport.rs` — `RusshTransport`; `crates/ssh-transport/src/local.rs` — `LocalTransport`; `crates/ssh-transport/src/sftp.rs`; `crates/ssh-transport/src/host_key.rs` |
| `crates/persistence-sqlite/` | SQLite profile and policy stores, migrations, encryption | `crates/persistence-sqlite/src/store.rs`; `crates/persistence-sqlite/src/policy_store.rs`; `crates/persistence-sqlite/src/extension_db.rs` — `ExtensionDb`; `crates/persistence-sqlite/src/extension_store.rs` — `ExtensionStore`; `crates/persistence-sqlite/src/encryption.rs` |
| `crates/credentials-keyring/` | OS keychain credential store | `crates/credentials-keyring/src/lib.rs` — `KeyringCredentialStore` |
| `crates/app-logic/` | Tauri-free application logic: chat-turn orchestration, confirmation, server/group management, DTOs | `crates/app-logic/src/state.rs` — `AppState`; `crates/app-logic/src/session.rs` — `Session`, `ChatTurnState`; `crates/app-logic/src/error.rs` — `CommandError`; `crates/app-logic/src/confirmation.rs` — `ConfirmationRegistry`; `crates/app-logic/src/servers.rs` — `create_server` |
| `crates/app-shell/` | Tauri layer: commands, events, wiring | `crates/app-shell/src/lib.rs` — `run`; `crates/app-shell/src/wiring.rs` — `Wiring`; `crates/app-shell/src/commands/` — all `#[tauri::command]`s |
| `crates/mcp-server/` | MCP server that exposes sessions to external clients | `crates/mcp-server/src/tool_server.rs` — `SmartSshMcpServer`, `ProposeCommandArgs`; `crates/mcp-server/src/auth.rs` — `require_bearer_token`, `SharedToken` |
| `crates/speech/` | Speech-to-text | `crates/speech/src/lib.rs` — `ModelManager`; `crates/speech/src/transcribe.rs` — `strip_non_speech` |
| `crates/tls-config/` | Shared TLS configuration | `crates/tls-config/src/lib.rs` |
| `crates/tauri-plugin-android-host/` | Android library module (session service, microphone permission) for apps embedding app-shell | `crates/tauri-plugin-android-host/src/lib.rs`, `crates/tauri-plugin-android-host/android/` |
| `crates/real-app-tests/` | Starts the real app in CI | `crates/real-app-tests/tests/` |
| `apps/smart-ssh-community/` | Thin binary, `app_shell::run(Wiring::community())` | `apps/smart-ssh-community/src/lib.rs`; `apps/smart-ssh-community/tauri.conf.json` |

## 2. From chat message to command on the server

1. Frontend: `apps/smart-ssh-community/frontend/src/api.ts` — `sendChatMessage` (called from `apps/smart-ssh-community/frontend/src/components/ChatPanel.tsx` — `ChatPanel`).
2. Tauri command: `crates/app-shell/src/commands/chat.rs` — `send_chat_message`, `send_chat_message_impl`.
3. Chat turn: `crates/app-logic/src/orchestration/chat_turn.rs` — `run_chat_turn`, `run_one_round`; the AI answer arrives as AiEvent values (`crates/core/src/ai/types.rs` — `AiEvent`).
4. Action proposed: `crates/app-logic/src/orchestration/action_exec.rs` — `handle_action_proposed`, `evaluate_action`.
5. Filter engine: `crates/core/src/filter/engine.rs` — `FilterEngine::evaluate` (a rule-based deny ends the path).
6. Risk classifier: `crates/core/src/risk/classifier.rs` — `RuleBasedRiskClassifier`; the AI second opinion (escalation only) is `crates/app-logic/src/second_opinion.rs` — `fetch_second_opinion`.
7. Confirmation: `crates/app-logic/src/orchestration/action_exec.rs` — `handle_user_decision`, waiting on `crates/app-logic/src/confirmation.rs` — `ConfirmationRegistry`; the user answer enters through `crates/app-shell/src/commands/chat.rs`.
8. Execution: `crates/app-logic/src/orchestration/action_exec.rs` — `execute_action`, `execute_suggested_command`, which call `crates/core/src/ssh/transport.rs` — `SshTransport`; the output is redacted (`crates/core/src/ai/redactor.rs` — `OutputRedactor`) before it is stored or sent to the AI.
9. Transport: `crates/ssh-transport/src/transport.rs` — `RusshTransport` (or `crates/ssh-transport/src/local.rs` — `LocalTransport`).

External MCP clients enter at step 4 through `crates/app-logic/src/orchestration/action_exec.rs` — `handle_mcp_action_proposed`.

## 3. Frontend map

Root: `apps/smart-ssh-community/frontend/src/`.

| Area | Main modules |
|---|---|
| Bridge to the backend | `apps/smart-ssh-community/frontend/src/api.ts` (one `invoke` wrapper per command), `apps/smart-ssh-community/frontend/src/events.ts` — `onConnectionStatusChanged`, `apps/smart-ssh-community/frontend/src/types.ts` (DTO types), `apps/smart-ssh-community/frontend/src/errorCodes.ts` |
| App shell and navigation | `apps/smart-ssh-community/frontend/src/App.tsx`, `apps/smart-ssh-community/frontend/src/components/AppHeader.tsx`, `apps/smart-ssh-community/frontend/src/components/Sidebar.tsx`, `apps/smart-ssh-community/frontend/src/navEntries.ts` |
| Startup | `apps/smart-ssh-community/frontend/src/components/StartupGate.tsx`, `apps/smart-ssh-community/frontend/src/components/FirstRunNoticeScreen.tsx` |
| Sessions and tabs | `apps/smart-ssh-community/frontend/src/useSessionTabs.ts`, `apps/smart-ssh-community/frontend/src/components/SessionView.tsx`, `apps/smart-ssh-community/frontend/src/components/SessionTabBar.tsx`, `apps/smart-ssh-community/frontend/src/components/TerminalView.tsx` |
| Chat | `apps/smart-ssh-community/frontend/src/components/ChatPanel.tsx` — `ChatPanel`, `ChatItemView`; `apps/smart-ssh-community/frontend/src/components/QuestionCard.tsx`; `apps/smart-ssh-community/frontend/src/components/LiveCommandOutput.tsx` |
| Files | `apps/smart-ssh-community/frontend/src/components/FileBrowserPanel.tsx` — `FileBrowserPanel`; `apps/smart-ssh-community/frontend/src/useLocalEditSession.ts`; `apps/smart-ssh-community/frontend/src/remotePath.ts` |
| Notes | `apps/smart-ssh-community/frontend/src/components/NotesPanel.tsx`, `apps/smart-ssh-community/frontend/src/components/NoteDiffPreview.tsx` |
| Server and group management | `apps/smart-ssh-community/frontend/src/components/ManagementView.tsx`, `apps/smart-ssh-community/frontend/src/components/ServerForm.tsx` — `ServerForm`; `apps/smart-ssh-community/frontend/src/components/GroupForm.tsx`; `apps/smart-ssh-community/frontend/src/components/ServerList.tsx`; `apps/smart-ssh-community/frontend/src/groupTree.ts` |
| Filter rules | `apps/smart-ssh-community/frontend/src/components/FilterRulesView.tsx` |
| Settings | `apps/smart-ssh-community/frontend/src/components/SettingsScreen.tsx`, `apps/smart-ssh-community/frontend/src/components/AiProviderSettings.tsx`, `apps/smart-ssh-community/frontend/src/components/McpServerSettings.tsx`, `apps/smart-ssh-community/frontend/src/components/BackupSettings.tsx` |
| Messages and dialogs | `apps/smart-ssh-community/frontend/src/toastBus.ts`, `apps/smart-ssh-community/frontend/src/components/HostKeyDialog.tsx`, `apps/smart-ssh-community/frontend/src/components/ModalBackdrop.tsx`, `apps/smart-ssh-community/frontend/src/useDialogFocusTrap.ts` |
| Voice input | `apps/smart-ssh-community/frontend/src/useVoiceInput.ts`, `apps/smart-ssh-community/frontend/src/voiceApi.ts` |
| i18n and theme | `apps/smart-ssh-community/frontend/src/i18n.ts`, `apps/smart-ssh-community/frontend/src/locales/`, `apps/smart-ssh-community/frontend/src/theme/dark.css` |
| Extensions | `apps/smart-ssh-community/frontend/src/extensions/registry.ts`, `apps/smart-ssh-community/frontend/src/extensions/registerBuiltinExtensions.ts`, `apps/smart-ssh-community/frontend/src/extensions/entitlements.ts` |
| Browser tests | `apps/smart-ssh-community/frontend/e2e/harness/` (fake backend), `apps/smart-ssh-community/frontend/e2e/support/`, `apps/smart-ssh-community/frontend/e2e/tests/` |

## 4. Extension points

- `crates/app-shell/src/wiring.rs` — `Wiring`, `Wiring::community`: an edition assembles its stores, providers and extra commands here.
- `apps/smart-ssh-community/frontend/src/extensions/registry.ts` — `registerSettingsSection`, `registerDocumentAction`, `registerFirstRunNoticeExtension`: frontend contributions.
- `crates/persistence-sqlite/src/extension_db.rs` — `ExtensionDb`, `ExtensionMigration`, `validate_prefix`: namespaced tables with their own migrations.
- `crates/persistence-sqlite/src/extension_store.rs` — `ExtensionStore`, `NamespacedStore`: namespaced key-value store.
- `crates/core/src/entitlements.rs` — `Feature`, `EntitlementProvider`: which features an edition unlocks.

## 5. Where do I find …

| What | Where |
|---|---|
| Tauri command registration | `crates/app-shell/src/lib.rs` — `run` (the generate_handler list); consistency check: `scripts/check-tauri-commands.mjs` |
| Tauri commands, one file per topic | `crates/app-shell/src/commands/` — e.g. `crates/app-shell/src/commands/chat.rs`, `crates/app-shell/src/commands/servers.rs`, `crates/app-shell/src/commands/rules.rs` |
| Database migrations | `crates/persistence-sqlite/migrations/` (`NNNN_name.sql`, consecutive); applied in `crates/persistence-sqlite/src/store.rs` |
| Locale files | `apps/smart-ssh-community/frontend/src/locales/` (one folder per language); parity test `apps/smart-ssh-community/frontend/src/locales/localeKeyParity.test.ts` |
| Backend → frontend events | `crates/app-logic/src/events.rs` — `emit_data_changed`, `emit_connection_status_changed`; listeners in `apps/smart-ssh-community/frontend/src/events.ts` |
| DTOs sent to the frontend | `crates/app-logic/src/dto.rs` — `ServerDto`; TS side `apps/smart-ssh-community/frontend/src/types.ts` |
| Error type shown to the user | `crates/app-logic/src/error.rs` — `CommandError`; `apps/smart-ssh-community/frontend/src/errorCodes.ts` |
| Application state and sessions | `crates/app-logic/src/state.rs` — `AppState`; `crates/app-logic/src/session.rs` — `Session` |
| In-memory stores for tests | `crates/app-logic/src/test_support.rs` — `InMemoryProfileStore`; `crates/persistence-sqlite/src/test_support.rs` — `migrate_up_to` |
| Mock SSH transport and SFTP | `crates/core/src/ssh/mock.rs` (feature `test-support`) |
| Real SSH tests | `crates/ssh-transport/tests/integration.rs` |
| Elevated SFTP channel | `crates/app-shell/src/elevated_sftp.rs` — `ElevatedSftp`, `ElevatedSftpSlot`; sudo/sftp-server logic `crates/core/src/ssh/elevated.rs` — `is_safe_absolute_path`; commands `crates/app-shell/src/commands/elevation.rs` |
| SFTP commands | `crates/app-shell/src/commands/sftp.rs` — `sftp_list`, `sftp_download`, `sftp_upload`; transfers `crates/app-shell/src/commands/sftp/transfer.rs` |
| Browser access gate for file browsing | `crates/app-shell/src/commands/elevation.rs` — `BrowserAccess`, `BrowserChannel` |
| MCP server | `crates/mcp-server/src/tool_server.rs` — `SmartSshMcpServer`; backend glue `crates/app-shell/src/mcp_backend.rs`; settings `crates/app-shell/src/mcp_settings.rs` |
| Speech input | `crates/speech/src/lib.rs` — `ModelManager`; Tauri side `crates/app-shell/src/voice_input.rs` — `get_speech_language`; models `crates/app-shell/src/speech_models.rs` |
| Android background service | `crates/app-shell/src/android_service.rs` (Rust), `crates/tauri-plugin-android-host/android/` (Kotlin); build guide `docs/android-build.md` |
| Local pseudo-server | `crates/app-shell/src/local_server.rs` — `synthetic_server`; `crates/ssh-transport/src/local.rs` — `LocalTransport` |
| Filter rules storage and defaults | `crates/persistence-sqlite/src/policy_store.rs`; `crates/app-logic/src/policy.rs`; hard blacklist `crates/core/src/filter/blacklist.rs` |
| Output redaction | `crates/core/src/ai/redactor.rs` — `DefaultOutputRedactor`; `crates/core/src/ai/live_redaction.rs` |
| System prompt for the AI | `crates/app-logic/src/system_prompt.rs`; session context `crates/app-shell/src/commands/connect.rs` — `build_session_system_context` |
| Context compaction | `crates/app-logic/src/compaction.rs` |
| Server notes | `crates/app-logic/src/orchestration/notes.rs`; `crates/core/src/profiles/notes.rs` |
| Backup and restore | `crates/core/src/backup/format.rs`; `crates/persistence-sqlite/src/backup_store.rs`; `crates/app-shell/src/commands/backup.rs` |
| Master password and field encryption | `crates/app-logic/src/master_password.rs`; `crates/persistence-sqlite/src/encryption.rs`; `crates/core/src/crypto/key.rs` |
| Data paths on disk | `crates/app-logic/src/data_paths.rs`; `docs/datenpfade.md` |
| Outgoing network connections | `crates/core/src/outgoing.rs`; `docs/netzwerkverbindungen.md` |
| Build hash and build type | `crates/app-logic/src/version.rs`; `crates/app-shell/src/commands/app_meta.rs` — `get_app_info` |
| Architecture decisions | `docs/adr/`; specs `docs/specs/` |
| Changelog fragments | `changelog.d/README.md` |
| CI workflow | `.github/workflows/community.yml` |

## 6. Large files (more than 2,000 lines)

Production code:

- `apps/smart-ssh-community/frontend/src/components/FileBrowserPanel.tsx` — `FileBrowserPanel` is one component: listing and navigation, selection, context menu, upload/download/transfers, rename/delete/create, preview and edit, elevated mode; small helpers `formatMb`, `findMenuTrigger` sit around it.
- `apps/smart-ssh-community/frontend/src/components/ChatPanel.tsx` — `ChatPanel` (message list, input, queue, stop), `ChatItemView` (one chat item), `ConfirmActionForm` (confirmation card), `RiskBadges`, `ActionResultView`, `TakeIntoNoteButton`.
- `apps/smart-ssh-community/frontend/src/components/ServerForm.tsx` — `ServerForm` with tabs general, connection, security, notes; `authStateFromKind` and `toAuthMethodInput` map the auth form; `TestResultBadge`.
- `crates/ai-providers/src/anthropic.rs` — `AnthropicProvider`: request building (`anthropic_tool_definition`), stream parsing (`to_raw_stream`), retry (`RetryState`), web research citations.
- `crates/ai-providers/src/openai_compatible.rs` — `OpenAiCompatibleProvider`: request building, `connect_and_stream`, `process_frame_stream`, tool-call buffering (`OpenAiStreamState`).
- `crates/app-shell/src/commands/sftp.rs` — SFTP commands: listing (`sftp_list`), `sftp_download`, `sftp_upload`, mutations, edit sessions.
- `crates/app-logic/src/orchestration/action_exec.rs` — `handle_action_proposed` (entry), `evaluate_action` (filter plus risk), `handle_user_decision` (confirmation), `execute_action` and `execute_suggested_command` (run and redact), `check_for_injected_instructions`.
- `crates/core/src/filter/parser.rs` — shell-command parsing for the filter: `split_top_level_segments`, here-string and shell-wrapper analysis, `analyse_options`.
- `crates/app-logic/src/servers.rs` — server CRUD: `create_server`, `update_server`, `delete_server`, `move_server_to_group`.

Test files (the same areas): `crates/core/src/ai/tests.rs`, `crates/core/src/filter/tests.rs`, `crates/ssh-transport/tests/integration.rs`, `crates/app-logic/src/database_startup/tests.rs`, `crates/app-logic/src/orchestration/notes/tests.rs`, `crates/app-logic/src/orchestration/chat_turn/tests_continuation.rs`, `crates/app-logic/src/orchestration/chat_turn/tests_compaction.rs`, `crates/app-logic/src/orchestration/action_exec/tests_files_and_ledger.rs`, `crates/app-logic/src/orchestration/action_exec/tests_core.rs`, `crates/app-shell/src/commands/sftp/revocation_tests.rs`, `apps/smart-ssh-community/frontend/src/components/FileBrowserPanel.test.tsx`.

## 7. Pitfalls when extending

Recipes are grep commands in fenced blocks, so the checker leaves them alone.

- **New `Session` or `Server` profile field.** Many struct literals across several crates, plus store, backup, DTO and frontend sites. The full checklist with the grep commands is in [`docs/recipes/new-session-or-server-field.md`](recipes/new-session-or-server-field.md).
- **New AI provider type.** See [`docs/recipes/new-ai-provider.md`](recipes/new-ai-provider.md).
- **New parameter on a Tauri command.** If the `invoke` lacks it, every call fails: update the wrapper in `apps/smart-ssh-community/frontend/src/api.ts` at once (with a default) and adjust `toHaveBeenCalledWith(...)` expectations in tests.
- **New API or event function in the frontend.** Add it to every test that replaces the api or events module with `vi.mock`, otherwise you get `undefined is not a function`; mocks followed by `.catch` must return a promise.
- **New `AiEvent` variant.** Every `match event` gets an arm: the main round and the side calls in orchestration, compaction and the risk second opinion:

  ```bash
  grep -rn 'AiEvent::' --include='*.rs' crates | grep -v tests
  ```
- **Side calls to the AI** (title, note, summary, second opinion, injection check) set `max_tokens_hint = Some(SIDE_CALL_MAX_TOKENS)` — never inherit the large main-chat default.
- **New user-visible text.** Add the key to **every** locale folder in `apps/smart-ssh-community/frontend/src/locales/`; `apps/smart-ssh-community/frontend/src/locales/localeKeyParity.test.ts` fails when a locale lacks a key. See `docs/i18n-glossary.md`.
- **New Tauri command.** Register it in the generate_handler list in `crates/app-shell/src/lib.rs` and add a handler to `apps/smart-ssh-community/frontend/e2e/harness/fakeBackend.ts`; `npm run lint` checks the registration.
- **`cargo fmt` rewraps code.** Scripted text replacements on freshly formatted code may no longer match; read the current state first.
- **clippy `needless_borrow`** appears when a helper already has `&Session` and you pass `&session` again.
