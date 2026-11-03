import { useTranslation } from "react-i18next";
import { translateErrorCode } from "../../errorCodes";
import type { KeyFileFactsDto } from "../../types";

/** Spec 0076, B-3: zeigt den Vorab-Befund über eine Schlüsseldatei —
 * existiert sie, passen die Rechte, sieht sie wie ein OpenSSH-Schlüssel
 * aus, ist sie verschlüsselt. Ein Fehlbefund (`facts.problem`) hindert das
 * Speichern nicht, er wird nur angezeigt (B-3, letzter Satz). Eigene
 * Komponente statt Inline-JSX, damit `ServerForm` nicht noch länger wird —
 * reiner Anzeige-Baustein ohne eigenen Zustand. */
export function IdentityFileFacts({
  loading,
  facts,
  error,
  emptyPath,
  path,
}: {
  path: string;
  loading: boolean;
  facts: KeyFileFactsDto | null;
  /** spec-reviewer-Fund (Review dieses Schritts): der `inspect_key_file`-
   * Aufruf selbst kann scheitern (z. B. IPC-Fehler) — ohne dieses Flag war
   * das ununterscheidbar von "noch nicht geprüft" und verschwand
   * kommentarlos. */
  error: boolean;
  emptyPath: boolean;
}) {
  const { t } = useTranslation();
  if (emptyPath) return null;
  if (error) {
    return <p className="mt-1 text-xs text-warning-fg">{t("serverForm.identityFile.checkFailed")}</p>;
  }
  if (facts === null) {
    return loading ? (
      <p className="mt-1 text-xs text-fg-subtle">{t("serverForm.identityFile.checking")}</p>
    ) : null;
  }

  const problemLine = facts.problem ? (
    <li className="text-warning-fg">
      {/* Issue #667 / Spec 0109 C3: the file is missing on this device. */}
      {facts.problem.code === "KEY_FILE_NOT_FOUND"
        ? `${t("errors.KEY_FILE_NOT_FOUND_ON_DEVICE")}: ${path}`
        : translateErrorCode(t, facts.problem.code, facts.problem.message)}
    </li>
  ) : (
    <li className="text-success-fg">{t("serverForm.identityFile.looksValid")}</li>
  );

  // B-3 verlangt Existenz, Rechte, Gültigkeit UND Verschlüsselung — auch
  // wenn die Datei einen ungültigen Schlüssel enthält. Ohne `exists` ist
  // über Rechte oder Verschlüsselung nichts bekannt — dann bleibt es bei
  // der einen Zeile.
  if (!facts.exists) {
    return <ul className="mt-1 space-y-0.5 text-xs">{problemLine}</ul>;
  }

  // spec-reviewer-Fund (Review Runde 2): `permissions_too_open` ist in
  // `OsKeyFileReader::inspect` (`key_files.rs`) nur dann verlässlich
  // ermittelt, wenn entweder gar kein Problem vorliegt ODER das Problem
  // `KEY_FILE_INVALID_KEY` ist — das ist der EINZIGE Fehlercode, der
  // beweisbar erst NACH der Rechteprüfung entsteht (er kommt ausschließlich
  // aus `read_and_classify`, das schon einen erfolgreichen `Probe` voraus-
  // setzt). Für `NotARegularFile` wird die Prüfung nie erreicht, für
  // `NotReadable`/`TooLarge` ist nicht unterscheidbar, ob sie vor oder
  // nach der Rechteprüfung entstanden sind — das DTO trägt dafür keine
  // zusätzliche Information. Die erste Fassung dieser Komponente zeigte in
  // all diesen Fällen unbedingt „Rechte eng genug gesetzt" — eine
  // verharmlosende Behauptung über einen Wert, der nie gemessen wurde.
  const permissionsAreKnown = !facts.problem || facts.problem.code === "KEY_FILE_INVALID_KEY";

  return (
    <ul className="mt-1 space-y-0.5 text-xs">
      {problemLine}
      {permissionsAreKnown &&
        (facts.permissionsTooOpen ? (
          <li className="text-warning-fg">{t("serverForm.identityFile.permissionsTooOpen")}</li>
        ) : (
          <li className="text-fg-subtle">{t("serverForm.identityFile.permissionsOk")}</li>
        ))}
      {/* `encrypted` ist nur aussagekräftig, wenn der Schlüssel überhaupt
       * gültig ist (bei einem Problem steht das Feld immer auf `false`,
       * s. `crate::dto::KeyFileFactsDto::from`). */}
      {!facts.problem && (
        <li className="text-fg-subtle">
          {facts.encrypted
            ? t("serverForm.identityFile.encrypted")
            : t("serverForm.identityFile.notEncrypted")}
        </li>
      )}
    </ul>
  );
}
