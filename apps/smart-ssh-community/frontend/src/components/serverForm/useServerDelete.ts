import { useState } from "react";
import { useTranslation } from "react-i18next";
import { deleteServer } from "../../api/api";
import { commandErrorText } from "../../api/errorCodes";
import type { DeleteServerResult } from "../../api/types";

interface Options {
  /** `null` = the server is not saved yet (nothing to delete). */
  serverId: string | null;
  setError: (message: string | null) => void;
  onDeleted: () => void;
}

/** Two-step delete of the server form (Spec 0046, Fund 1): the first call
 * (`confirm: false`) only returns the preview, the second one deletes. */
export function useServerDelete({ serverId, setError, onDeleted }: Options) {
  const { t } = useTranslation();
  const [deleting, setDeleting] = useState(false);
  const [deletePreview, setDeletePreview] = useState<DeleteServerResult | null>(null);
  // Spec 0071, A17: Der Server ist gelöscht, aber mindestens ein Secret
  // konnte nicht aus dem Schlüsselbund entfernt werden. Die Einträge sind
  // jetzt verwaist (die Server-ID gibt es nicht mehr) — das muss der
  // Nutzer erfahren, bevor die Maske zugeht.
  const [secretsLeftBehind, setSecretsLeftBehind] = useState<string[] | null>(null);

  const handleDeleteClick = async () => {
    if (!serverId) return;
    setError(null);
    try {
      setDeletePreview(await deleteServer(serverId, false));
    } catch (err) {
      setError(commandErrorText(t, err));
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
      setError(commandErrorText(t, err));
    } finally {
      setDeleting(false);
    }
  };

  return {
    deleting,
    deletePreview,
    secretsLeftBehind,
    setDeletePreview,
    setSecretsLeftBehind,
    handleDeleteClick,
    handleConfirmDelete,
  };
}
