import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { convertIdentityFileToKeychain, inspectKeyFile } from "../../api";
import { commandErrorText } from "../../errorCodes";
import type { KeyFileFactsDto, ServerDto } from "../../types";

interface Options {
  serverId: string | null;
  /** Identity file path currently typed in the form; `""` unless the auth kind is "identity file". */
  draftPath: string;
  /** Whether the form's auth kind is "identity file". */
  draftIsIdentityFile: boolean;
  /** The saved server, `null` while creating or before loading. */
  loaded: ServerDto | null;
  /** Reloads the server after a successful conversion. */
  reloadServer: () => void;
  onSaved: () => void;
  /** Called by `finishKeepingPath` after the pending import prompt is closed (form bookkeeping). */
  onKeepPathFinished: () => void;
}

/** Identity-file facts for the draft and the saved path, the keychain
 * conversion (Spec 0076) and the key-import prompt state (Issue #667). */
export function useIdentityFile({
  serverId,
  draftPath,
  draftIsIdentityFile,
  loaded,
  reloadServer,
  onSaved,
  onKeepPathFinished,
}: Options) {
  const { t } = useTranslation();
  // Spec 0076, B-3: der Vorab-Befund über die im Formular gerade
  // eingetragene Schlüsseldatei — `null`, solange kein Pfad gesetzt ist
  // oder noch keine Antwort da ist. Ein Fehlbefund hindert das Speichern
  // nicht (B-3), er wird nur angezeigt.
  const [identityFacts, setIdentityFacts] = useState<KeyFileFactsDto | null>(null);
  const [identityFactsLoading, setIdentityFactsLoading] = useState(false);
  // A failed `inspect_key_file` call itself (not a finding about the file)
  // must not look like "nothing checked yet".
  const [identityFactsError, setIdentityFactsError] = useState(false);
  // Spec 0076, C-7: unabhängig vom obigen Entwurfszustand — bezieht sich
  // auf den tatsächlich GESPEICHERTEN Pfad (`loaded.identityFilePath`) und
  // entscheidet, ob der Überführen-Knopf wählbar ist.
  const [convertFacts, setConvertFacts] = useState<KeyFileFactsDto | null>(null);
  const [convertFactsError, setConvertFactsError] = useState(false);
  const [convertConfirmOpen, setConvertConfirmOpen] = useState(false);
  const [converting, setConverting] = useState(false);
  const [convertError, setConvertError] = useState<string | null>(null);
  // Issue #667: dialog asking whether to import a key file on save.
  const [importPromptOpen, setImportPromptOpen] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
  // Set once the server is saved while the import is still pending/failed,
  // so a retry never creates the server a second time.
  const [importSavedId, setImportSavedId] = useState<string | null>(null);

  // Spec 0076, B-3: fragt `inspect_key_file` für den Pfad an, der gerade im
  // Formular steht — entkoppelt, gedämpft (400 ms), damit nicht jeder
  // Tastenanschlag einen eigenen Aufruf auslöst. Läuft nur, solange die
  // Anmeldeart "Schlüsseldatei" ist; ein leerer Pfad ruft gar nicht erst an
  // (die Datei darf erst später entstehen, B-3).
  useEffect(() => {
    // Reset at once on a path change: otherwise the finding of the previous
    // path would stay visible until the debounce elapses.
    setIdentityFacts(null);
    setIdentityFactsError(false);
    if (!draftIsIdentityFile || draftPath.trim() === "") {
      setIdentityFactsLoading(false);
      return;
    }
    let cancelled = false;
    setIdentityFactsLoading(true);
    const timer = setTimeout(() => {
      inspectKeyFile(draftPath)
        .then((facts) => {
          if (!cancelled) setIdentityFacts(facts);
        })
        .catch(() => {
          if (!cancelled) setIdentityFactsError(true);
        })
        .finally(() => {
          if (!cancelled) setIdentityFactsLoading(false);
        });
    }, 400);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [draftIsIdentityFile, draftPath]);

  // Spec 0076, C-7: derselbe Befund für den GESPEICHERTEN Pfad — unabhängig
  // davon, was der Nutzer gerade im Formular entwirft (der Knopf wirkt auf
  // den Server in der Datenbank, nicht auf den Entwurf).
  useEffect(() => {
    setConvertFacts(null);
    setConvertFactsError(false);
    if (!loaded || loaded.authKind !== "identity_file" || !loaded.identityFilePath) {
      return;
    }
    let cancelled = false;
    inspectKeyFile(loaded.identityFilePath)
      .then((facts) => {
        if (!cancelled) setConvertFacts(facts);
      })
      .catch(() => {
        if (!cancelled) setConvertFactsError(true);
      });
    return () => {
      cancelled = true;
    };
  }, [loaded]);

  /** Closes the conversion confirmation and clears its state (server reload / cancel). */
  const resetConvert = () => {
    setConvertConfirmOpen(false);
    setConverting(false);
    setConvertError(null);
  };

  // Spec 0076, C-1/C-3 (BL-0222): der eigentliche Übernahme-Aufruf, erst
  // nach dem Bestätigungsdialog aus C-2. Bei Erfolg wird der Server neu
  // geladen (die Anmeldeart hat sich geändert) und `onSaved` informiert den
  // Aufrufer wie nach jedem anderen Speichern.
  const handleConvertToKeychain = async () => {
    if (!serverId) return;
    setConverting(true);
    setConvertError(null);
    try {
      await convertIdentityFileToKeychain(serverId);
      setConvertConfirmOpen(false);
      reloadServer();
      onSaved();
    } catch (err) {
      setConvertError(commandErrorText(t, err));
    } finally {
      setConverting(false);
    }
  };

  /** "Keep file path" after the server was already saved by a failed import. */
  const finishKeepingPath = () => {
    setImportPromptOpen(false);
    setImportSavedId(null);
    onKeepPathFinished();
  };

  return {
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
  };
}
