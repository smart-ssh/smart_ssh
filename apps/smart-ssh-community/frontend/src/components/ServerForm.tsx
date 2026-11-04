import { useEffect, useMemo, useRef, useState } from "react";
import { onNoteShrinkSucceeded } from "../events";
import type { GroupDto, ServerDto } from "../types";
import type { MovedTo } from "./GroupForm";
import { LocalServerForm } from "./serverForm/LocalServerForm";
import { RemoteServerForm } from "./serverForm/RemoteServerForm";
import { TestResultBadge } from "./serverForm/TestResultBadge";
import { useConnectionTest } from "./serverForm/useConnectionTest";
import { useIdentityFile } from "./serverForm/useIdentityFile";
import { useServerDelete } from "./serverForm/useServerDelete";
import { useServerFormModel } from "./serverForm/useServerFormModel";
import { useServerFormTabs } from "./serverForm/useServerFormTabs";

export { TestResultBadge };

interface ServerFormProps {
  /** `null` = Neuanlage. */
  serverId: string | null;
  defaultGroupId: string | null;
  allGroups: GroupDto[];
  allServers: ServerDto[];
  onSaved: () => void;
  onDeleted: () => void;
  /** Issue #692: shows "Connect" next to Save for a saved server. */
  onConnect?: (serverId: string) => void;
  /** Spec 0058, Teil 2 — s. `NotesPanel.autoFocus`-Doc-Kommentar. Gilt für
   * BEIDE Notiz-Editoren dieses Formulars (den lokalen Pseudo-Server-Zweig
   * unten und den regulären `NotesPanel`-Zweig) — "Mache ich selbst" kann
   * sich auf jeden Server beziehen, den lokalen eingeschlossen. */
  autoFocusNotes?: boolean;
  /** Issue #63: s. `MovedTo` in `GroupForm.tsx`. `null`/fehlend = nicht
   * verschoben. */
  movedTo?: MovedTo | null;
}


/** Picks the local pseudo-server form or the remote server form by server
 * kind and owns what both share: the field model, the tabs and the
 * connection test, delete and key-file state of the remote form. */
export function ServerForm({
  serverId,
  defaultGroupId,
  allGroups,
  allServers,
  onSaved,
  onDeleted,
  onConnect,
  autoFocusNotes = false,
  movedTo = null,
}: ServerFormProps) {
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ text: string | null; loading: boolean }>({
    text: null,
    loading: false,
  });
  const formRef = useRef<HTMLFormElement>(null);
  const model = useServerFormModel({
    serverId,
    defaultGroupId,
    // The form's own transient state is reset on every (re)load.
    onBeforeLoad: () => {
      setError(null);
      test.resetResult();
      setPreview({ text: null, loading: false });
      del.setDeletePreview(null);
      // Without this reset the confirmation boundary of the key conversion
      // would rely on `ManagementView`'s `key={id}` remounting the form.
      identity.resetConvert();
    },
    onLoadError: setError,
  });
  const { loaded, isLocal, setGroupId, loadServer, buildInput, markSaved } = model;
  const tabs = useServerFormTabs({ serverId, isLocal, loaded, autoFocusNotes, formRef });
  const test = useConnectionTest({ serverId, buildInput, setError });
  const del = useServerDelete({ serverId, setError, onDeleted });
  const identity = useIdentityFile({
    serverId,
    draftIsIdentityFile: model.auth.kind === "identityFile",
    draftPath: model.auth.kind === "identityFile" ? model.auth.path : "",
    loaded,
    reloadServer: () => loadServer(),
    onSaved,
    onKeepPathFinished: () => {
      markSaved();
      tabs.setInvalidTabs(new Set());
      onSaved();
    },
  });

  // Issue #63: after a drag-and-drop move of this server only the group is
  // taken over; every other unsaved input stays.
  useEffect(() => {
    if (movedTo) setGroupId(movedTo.groupId);
  }, [movedTo, setGroupId]);

  // An accepted note summary can arrive while this form is open; without a
  // reload a later save would overwrite it with the old `localNotes` draft.
  useEffect(() => {
    if (!loaded) return;
    const unlisten = onNoteShrinkSucceeded((event) => {
      if (event.serverId === loaded.id) loadServer();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- listener is re-registered per loaded server; loadServer is read at event time
  }, [loaded?.id]);

  // Spec 0032, section 6: the local pseudo-server cannot be a jump host.
  const possibleJumpHosts = useMemo(
    () => allServers.filter((s) => s.id !== serverId && !s.isLocal),
    [allServers, serverId],
  );

  if (loaded && isLocal) {
    return (
      <LocalServerForm
        loaded={loaded}
        model={model}
        tabs={tabs}
        error={error}
        setError={setError}
        onSaved={onSaved}
        autoFocusNotes={autoFocusNotes}
      />
    );
  }
  return (
    <RemoteServerForm
      serverId={serverId}
      allGroups={allGroups}
      possibleJumpHosts={possibleJumpHosts}
      model={model}
      tabs={tabs}
      test={test}
      del={del}
      identity={identity}
      formRef={formRef}
      error={error}
      setError={setError}
      preview={preview}
      setPreview={setPreview}
      onSaved={onSaved}
      onDeleted={onDeleted}
      onConnect={onConnect}
      autoFocusNotes={autoFocusNotes}
    />
  );
}
