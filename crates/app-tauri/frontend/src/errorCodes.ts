/** Spec 0024, Abschnitt 5: Mapping von stabilen Backend-`code`s (s.
 * `SshError`/`AiError`/`Decision`/`CommandError` in `crates/core`/
 * `crates/app-tauri`) auf Übersetzungs-Keys im `errors`-Namespace der
 * `locales/*\/common.json`. Jeder bekannte Code übersetzt 1:1 zu
 * `errors.<CODE>` — die Menge hier definiert, welche Codes das Frontend
 * kennt; alles andere (unbekannter/zukünftiger Code, oder gar keiner) fällt
 * auf den mitgegebenen `fallback`-Text zurück, nie eine leere Anzeige. */
const KNOWN_ERROR_CODES = new Set<string>([
  // SshError (crates/core/src/ssh/error.rs)
  "SSH_CONNECTION_FAILED",
  "SSH_AUTH_FAILED",
  "SSH_HOST_KEY_REJECTED",
  "SSH_CHANNEL_ERROR",
  "SSH_TIMEOUT",
  "SSH_JUMP_HOST_CYCLE",
  "SSH_CREDENTIAL_RESOLUTION_FAILED",
  "SSH_SFTP_PERMISSION_DENIED",
  // AiError (crates/core/src/ai/types.rs)
  "AI_AUTH_FAILED",
  "AI_RATE_LIMITED",
  "AI_NETWORK_ERROR",
  "AI_INVALID_RESPONSE",
  "AI_CONTEXT_TOO_LARGE",
  "AI_PROVIDER_UNAVAILABLE",
  // Decision/EvaluationTrace (crates/core/src/filter/engine.rs +
  // crates/app-tauri/src/orchestration.rs)
  "FILTER_EMPTY_COMMAND",
  "FILTER_COMMAND_TOO_LONG",
  "FILTER_PARSE_AMBIGUOUS",
  "FILTER_HARD_BLACKLIST",
  "FILTER_OUTPUT_REDIRECTION",
  "FILTER_COMMAND_SUBSTITUTION",
  "FILTER_RULE_DENY",
  "FILTER_RULE_CONFIRM",
  "FILTER_NO_RULE_MATCHED",
  "FILTER_AUTO_CONTINUATION_REQUIRES_CONFIRM",
  "FILTER_MCP_ORIGIN_REQUIRES_CONFIRM",
  "FILTER_SUDO_PASSWORD_REQUIRES_CONFIRM",
  "FILTER_NOTE_UPDATE_REQUIRES_CONFIRM",
  "FILTER_FILE_WRITE_REQUIRES_CONFIRM",
  // CommandError (crates/app-tauri/src/error.rs) — Server-/Gruppen-Formulare
  "GROUP_SELF_PARENT",
  "GROUP_CYCLE_DETECTED",
  "SERVER_PASSWORD_REQUIRED",
  "SERVER_PRIVATE_KEY_REQUIRED",
  "SERVER_CERTIFICATE_REQUIRED",
  "SERVER_CERTIFICATE_KEY_REQUIRED",
  "SERVER_JUMP_HOST_LOCAL",
]);

/** Übersetzt `code` über den `errors`-Namespace, fällt bei `null`/
 * `undefined`/unbekanntem Code auf `fallback` zurück (den bestehenden
 * `Display`-Text des Backend-Fehlers) — nie eine leere Anzeige (Spec 0024,
 * Abschnitt 5). */
export function translateErrorCode(
  t: (key: string, options?: Record<string, unknown>) => string,
  code: string | null | undefined,
  fallback: string,
): string {
  if (!code || !KNOWN_ERROR_CODES.has(code)) return fallback;
  return t(`errors.${code}`);
}
