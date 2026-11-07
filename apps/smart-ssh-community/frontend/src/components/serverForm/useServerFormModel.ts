import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { clearServerSudoPassword, commandErrorMessage, getServer, listStoredHostKeys } from "../../api/api";
import { loadRiskClassifierSettings } from "../../settings/riskSettings";
import { checkStartDirectory } from "../../lib/startDirectory";
import type { PostIngestPolicy, ServerInput, StoredHostKeyDto } from "../../api/types";
import {
  type AuthFormState,
  type ServerFormFieldKey,
  type ServerFormState,
  initialServerFormState,
  serverFormReducer,
  toAuthMethodInput,
} from "./serverFormModel";

interface Options {
  /** `null` = create mode. */
  serverId: string | null;
  defaultGroupId: string | null;
  /** Called at the start of every (re)load, before the backend is asked:
   * the form resets its own transient state here. */
  onBeforeLoad: () => void;
  onLoadError: (message: string) => void;
}

/** Owns every field value of the server form, loading a server, building
 * the save input and the dirty flag (reducer in `serverFormModel.ts`). */
export function useServerFormModel({ serverId, defaultGroupId, onBeforeLoad, onLoadError }: Options) {
  const isCreate = serverId === null;
  const [state, dispatch] = useReducer(serverFormReducer, defaultGroupId, initialServerFormState);
  const [storedHostKeys, setStoredHostKeys] = useState<StoredHostKeyDto[] | null>(null);
  // Spec 0039, Abschnitt 5.2: die Checkbox ist nur bedienbar, wenn ein
  // Zweitmeinungs-Provider konfiguriert ist (dieselbe Voraussetzung wie
  // beim Backend-`Session::injection_check_provider`) — sonst bliebe die
  // Einstellung wirkungslos, ohne dass das sichtbar wäre.
  const [secondOpinionAvailable, setSecondOpinionAvailable] = useState(false);
  const [clearingSudoPassword, setClearingSudoPassword] = useState(false);
  useEffect(() => {
    loadRiskClassifierSettings()
      .then((settings) => setSecondOpinionAvailable(settings.enabled && settings.providerId !== null))
      .catch(() => setSecondOpinionAvailable(false));
  }, []);
  const callbacks = useRef({ onBeforeLoad, onLoadError });
  callbacks.current = { onBeforeLoad, onLoadError };

  const loadServer = useCallback(() => {
    callbacks.current.onBeforeLoad();
    setStoredHostKeys(null);
    if (serverId === null) {
      dispatch({ type: "reset", defaultGroupId });
      return;
    }
    listStoredHostKeys(serverId)
      .then((keys) => setStoredHostKeys(keys))
      .catch(() => setStoredHostKeys(null));
    getServer(serverId)
      .then((server) => dispatch({ type: "load", server }))
      .catch((err) => callbacks.current.onLoadError(commandErrorMessage(err)));
  }, [serverId, defaultGroupId]);

  useEffect(loadServer, [loadServer]);

  const setField = useCallback(
    <K extends ServerFormFieldKey>(field: K, value: ServerFormState[K]) =>
      dispatch({ type: "setField", field, value }),
    [],
  );

  const isLocal = state.loaded?.isLocal ?? false;
  const startDirectoryCheck = checkStartDirectory(state.startDirectory);

  const buildInput = (): ServerInput => ({
    name: state.name,
    host: state.host,
    port: Number(state.port),
    username: state.username,
    groupId: state.groupId,
    tags: state.tags,
    auth: toAuthMethodInput(state.auth, isCreate),
    jumpHost: state.jumpHost,
    sudoPassword: state.sudoPassword === "" ? null : state.sudoPassword,
    postIngestPolicy: state.postIngestPolicy,
    // Unabhängiger Review-Pass (Spec 0039): NICHT zusätzlich mit
    // `secondOpinionAvailable` verknüpfen — sonst würde das Speichern eines
    // Servers aus einem völlig anderen Grund (z. B. Umbenennen), während
    // gerade kein Zweitmeinungs-Provider konfiguriert ist, eine zuvor
    // gesetzte Einstellung still auf `false` zurücksetzen und beim späteren
    // erneuten Konfigurieren des Providers nicht automatisch wiederkommen.
    // Die Wirksamkeitsprüfung (Provider konfiguriert?) macht ohnehin das
    // Backend (`Session::injection_check_provider`); die Checkbox hier ist
    // nur bedienbar, nicht das gespeicherte Feld selbst gegated.
    aiInjectionCheckEnabled: state.aiInjectionCheckEnabled,
    sftpServerPath: state.sftpServerPath.trim() === "" ? null : state.sftpServerPath.trim(),
    elevationAllowed: state.elevationAllowed,
    // Spec 0102: der lokale Pseudo-Server bietet das Feld nicht an.
    startDirectory: isLocal || !startDirectoryCheck.ok ? null : startDirectoryCheck.value,
  });

  return {
    ...state,
    isLocal,
    isCreate,
    storedHostKeys,
    startDirectoryCheck,
    loadServer,
    buildInput,
    setField,
    setName: (v: string) => setField("name", v),
    setHost: (v: string) => setField("host", v),
    setPort: (v: string) => setField("port", v),
    setUsername: (v: string) => setField("username", v),
    setGroupId: useCallback((v: string | null) => dispatch({ type: "setField", field: "groupId", value: v }), []),
    setJumpHost: (v: string | null) => setField("jumpHost", v),
    setSudoPassword: (v: string) => setField("sudoPassword", v),
    setPostIngestPolicy: (v: PostIngestPolicy) => setField("postIngestPolicy", v),
    setAiInjectionCheckEnabled: (v: boolean) => setField("aiInjectionCheckEnabled", v),
    setElevationAllowed: (v: boolean) => setField("elevationAllowed", v),
    setSftpServerPath: (v: string) => setField("sftpServerPath", v),
    setStartDirectory: (v: string) => setField("startDirectory", v),
    setLocalNotes: (v: string) => setField("localNotes", v),
    setAuth: (auth: AuthFormState) => dispatch({ type: "setAuth", auth }),
    setTags: (tags: string[]) => dispatch({ type: "setTags", tags }),
    markDirty: useCallback(() => dispatch({ type: "markDirty" }), []),
    markSaved: useCallback(() => dispatch({ type: "saved" }), []),
    secondOpinionAvailable,
    clearingSudoPassword,
    /** Removes the stored sudo password (Spec 0018, section 4); `onError`
     * receives the failure, the draft flags only change on success. */
    clearSudoPassword: async (onError: (err: unknown) => void) => {
      if (!serverId) return;
      setClearingSudoPassword(true);
      try {
        await clearServerSudoPassword(serverId);
        setField("hasSudoPassword", false);
        setField("sudoPasswordUnknown", false);
      } catch (err) {
        onError(err);
      } finally {
        setClearingSudoPassword(false);
      }
    },
  };
}
