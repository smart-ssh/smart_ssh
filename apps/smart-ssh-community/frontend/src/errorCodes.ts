/** Spec 0024, Abschnitt 5: Mapping von stabilen Backend-`code`s (s.
 * `SshError`/`AiError`/`Decision`/`CommandError` in `crates/core`/
 * `crates/app-shell`) auf Übersetzungs-Keys im `errors`-Namespace der
 * `locales/*\/common.json`. Jeder bekannte Code übersetzt 1:1 zu
 * `errors.<CODE>` — die Menge hier definiert, welche Codes das Frontend
 * kennt; alles andere (unbekannter/zukünftiger Code, oder gar keiner) fällt
 * auf den mitgegebenen `fallback`-Text zurück, nie eine leere Anzeige. */
/** Spec 0069, Teil A1: die Codes des Fünf-Minuten-Pfads (Provider
 * einrichten → Server anlegen → Verbindung testen → verbinden → Frage
 * stellen) — jeder hier gelistete Code hat DE/EN-Text mit Ursache und
 * nächstem Schritt (geprüft von `errorCodes.test.ts`/`spec-reviewer`) UND
 * steht in `KNOWN_ERROR_CODES` unten. Bewusst als eigene, geprüfte Liste
 * statt implizit "alles in `KNOWN_ERROR_CODES`" — die größere Liste
 * enthält auch Filter-/Formular-Codes außerhalb dieses Pfads, für die die
 * "Ursache + nächster Schritt"-Anforderung nicht gilt. */
export const FIVE_MINUTE_PATH_ERROR_CODES = [
  "AI_AUTH_FAILED",
  "AI_MODEL_NOT_FOUND",
  "AI_LOCAL_PROVIDER_UNREACHABLE",
  "AI_NETWORK_ERROR",
  "AI_TIMEOUT",
  "AI_PROVIDER_UNAVAILABLE",
  "AI_RATE_LIMITED",
  "AI_NO_ACTIVE_PROVIDER",
  "SSH_CONNECTION_REFUSED",
  "SSH_HOST_NOT_FOUND",
  "SSH_HOST_UNREACHABLE",
  "SSH_TIMEOUT",
  "SSH_CONNECTION_CLOSED",
  "SSH_CONNECTION_FAILED",
  "SSH_AUTH_FAILED",
  "SSH_HOST_KEY_NOT_TRUSTED",
  "SSH_HOST_KEY_CONFIRM_TIMEOUT",
] as const;

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
  // Spec 0069, Teil A3:
  "SSH_CONNECTION_REFUSED",
  "SSH_HOST_NOT_FOUND",
  "SSH_HOST_UNREACHABLE",
  "SSH_CONNECTION_CLOSED",
  // Spec 0069, Teil A4:
  "SSH_HOST_KEY_NOT_TRUSTED",
  "SSH_HOST_KEY_CONFIRM_TIMEOUT",
  // AiError (crates/core/src/ai/types.rs)
  "AI_AUTH_FAILED",
  "AI_RATE_LIMITED",
  "AI_NETWORK_ERROR",
  "AI_INVALID_RESPONSE",
  "AI_CONTEXT_TOO_LARGE",
  "AI_PROVIDER_UNAVAILABLE",
  // Spec 0065, Teil 3 (spec-reviewer-Fund, Review dieses Schritts): fehlte
  // hier — die englische UI zeigte bislang den rohen deutschen
  // Backend-Text statt einer Übersetzung.
  "AI_RESPONSE_TRUNCATED",
  // Spec 0069, Teil A2:
  "AI_MODEL_NOT_FOUND",
  "AI_LOCAL_PROVIDER_UNREACHABLE",
  "AI_TIMEOUT",
  // Spec 0069, Teil A4:
  "AI_NO_ACTIVE_PROVIDER",
  // Decision/EvaluationTrace (crates/core/src/filter/engine.rs +
  // crates/app-shell/src/orchestration.rs)
  "FILTER_EMPTY_COMMAND",
  "FILTER_COMMAND_TOO_LONG",
  "FILTER_PARSE_AMBIGUOUS",
  "FILTER_HARD_BLACKLIST",
  "FILTER_OUTPUT_REDIRECTION",
  "FILTER_COMMAND_SUBSTITUTION",
  "FILTER_SUBSTITUTION_TOO_DEEP",
  "FILTER_RULE_DENY",
  "FILTER_RULE_CONFIRM",
  "FILTER_NO_RULE_MATCHED",
  "FILTER_POST_INGEST_REQUIRES_CONFIRM",
  "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM",
  "FILTER_MCP_ORIGIN_REQUIRES_CONFIRM",
  "FILTER_SUDO_PASSWORD_REQUIRES_CONFIRM",
  "FILTER_SECRET_PATH_READ_REQUIRES_CONFIRM",
  "FILTER_EARLIER_ACTION_REJECTED_REQUIRES_CONFIRM",
  "FILTER_SFTP_SERVER_REQUIRES_CONFIRM",
  "FILTER_NOTE_UPDATE_REQUIRES_CONFIRM",
  "FILTER_FILE_WRITE_REQUIRES_CONFIRM",
  // CommandError (crates/app-shell/src/error.rs) — Server-/Gruppen-Formulare
  "GROUP_SELF_PARENT",
  "GROUP_CYCLE_DETECTED",
  "SERVER_PASSWORD_REQUIRED",
  "SERVER_PRIVATE_KEY_REQUIRED",
  "SERVER_CERTIFICATE_REQUIRED",
  "SERVER_CERTIFICATE_KEY_REQUIRED",
  "SERVER_JUMP_HOST_LOCAL",
  // Unabhängiger Review-Pass, Spec 0031: Code existiert seit dem
  // First-Run-Notice-Gate in error.rs (code_tests), war aber nie hier
  // eingetragen — ohne diesen Eintrag sah selbst ein Nutzer mit englischer
  // UI in dem (seltenen) Fall, dass der Backend-Fehler tatsächlich sichtbar
  // wird, den rohen deutschen Text.
  "FIRST_RUN_NOTICE_NOT_ACKNOWLEDGED",
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
