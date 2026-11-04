import { useState, type MutableRefObject } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage } from "../../api";
import { showToast } from "../../toastBus";
import type { RemoteEntryDto } from "../../types";
import { useLocalEditSession, type UploadOffer } from "../../hooks/useLocalEditSession";
import { currentCaps } from "./shared";
import type { NotifyFailed, NotifyOk } from "./useFileNotify";

interface UseLocalEditFlowOptions {
  elevated: boolean;
  elevatedUser: string | null;
  channelUser: string | null;
  path: string;
  load: (path: string) => void;
  setError: (message: string) => void;
  notifyOk: NotifyOk;
  notifyFailed: NotifyFailed;
  closeMenu: () => void;
  editUploadingNameRef: MutableRefObject<string | null>;
}

/**
 * Spec 0054, Teil 4: "Lokal öffnen -> bearbeiten -> Upload anbieten". Wraps
 * `useLocalEditSession` with the dialog state of the flow (upload offer,
 * identity chosen for the upload, in-app editor).
 */
export function useLocalEditFlow(
  sessionId: string,
  {
    elevated,
    elevatedUser,
    channelUser,
    path,
    load,
    setError,
    notifyOk,
    notifyFailed,
    closeMenu,
    editUploadingNameRef,
  }: UseLocalEditFlowOptions,
) {
  const { t } = useTranslation();
  const localEdit = useLocalEditSession(sessionId);
  const [editUploadOffer, setEditUploadOffer] = useState<UploadOffer | null>(null);
  // Issue #679: the app's own editor on the edit copy (no external program).
  const [inAppEditorOpen, setInAppEditorOpen] = useState(false);
  // Issue #656: the identity of the edit session differs from the active one
  // (other target user, or elevated mode switched on/off). Plain "Upload" is
  // not offered then; the explicit "Upload as <user>" click is the only way
  // to send the current identity.
  const editIdentityMismatch =
    localEdit.session !== null && localEdit.session.elevatedUser !== channelUser;
  // Identity chosen by that click, held until the offer dialog is confirmed.
  const [editUploadAs, setEditUploadAs] = useState<{ user: string | null } | null>(null);

  /** Spec 0054, Teil 4, Punkt 1/2: Download in den Editier-Temp-Ordner +
   * Öffnen mit dem konfigurierten (oder OS-Standard-)Programm. Fehler
   * (Download gescheitert, Programm nicht gefunden) landen in der
   * bestehenden `error`-Anzeige — der Hook selbst zeigt keine UI. */
  const handleOpenLocally = (entry: RemoteEntryDto) => {
    closeMenu();
    localEdit
      .startEditing(entry, elevated ? elevatedUser : null)
      .then(() => {
        if (currentCaps().externalEditor) notifyOk("fileToasts.openedLocally", { name: entry.name });
        else setInAppEditorOpen(true);
      })
      .catch((err) => notifyFailed("fileToasts.openLocallyFailed", entry.name, err));
  };

  /** Spec 0054, Teil 4, Punkt 5: baut die Diff-/Konflikt-Daten und öffnet
   * den Bestätigungsdialog — der eigentliche Upload passiert erst nach
   * Bestätigung in `handleConfirmEditUpload`. */
  const handleOfferEditUpload = (asOther = false) => {
    const target = asOther ? { user: channelUser } : null;
    setEditUploadAs(target);
    localEdit
      .buildUploadOffer(target ? target.user : undefined)
      .then((offer) => setEditUploadOffer(offer))
      .catch((err) => setError(commandErrorMessage(err)));
  };

  const closeEditUploadOffer = () => {
    setEditUploadOffer(null);
    setEditUploadAs(null);
  };

  const handleConfirmEditUpload = () => {
    setEditUploadOffer(null);
    const name = localEdit.session?.entry.name ?? "";
    // Spec-Reviewer-Fund: der Modus-Zusatz kommt von der Bearbeitung (so
    // wurde hochgeladen), nicht vom aktuellen Modus der Ansicht.
    const editUser = editUploadAs ? editUploadAs.user : (localEdit.session?.elevatedUser ?? null);
    const asOther = editUploadAs;
    setEditUploadAs(null);
    const editSuffix = editUser ? t("fileToasts.asElevated", { user: editUser }) : "";
    editUploadingNameRef.current = name;
    localEdit.confirmUpload(asOther ? asOther.user : undefined).then((error) => {
      editUploadingNameRef.current = null;
      // Issue #655: a failure is shown only in the edit footer (no toast).
      if (error === null) {
        showToast({ kind: "success", message: t("fileToasts.editUploaded", { name }) + editSuffix });
      }
      load(path);
    });
  };

  return {
    localEdit,
    editUploadOffer,
    editUploadAs,
    inAppEditorOpen,
    setInAppEditorOpen,
    editIdentityMismatch,
    handleOpenLocally,
    handleOfferEditUpload,
    closeEditUploadOffer,
    handleConfirmEditUpload,
  };
}
