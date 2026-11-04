import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, sftpList, sftpStartDirectory } from "../../api";
import { notifyMissingStartDirectory } from "../../startDirectory";
import type { RemoteEntryDto } from "../../types";

/**
 * Loading and navigating one remote directory of a session: current path,
 * the path input, the start directory (Spec 0102), the entries and the
 * loading/error state. `channelUser` is the expected user of the elevated
 * channel (`null` = normal channel, Spec 0067, A5).
 */
export function useRemoteDirectory(
  sessionId: string,
  channelUser: string | null,
) {
  const { t } = useTranslation();
  const [path, setPath] = useState(".");
  const [pathInput, setPathInput] = useState(".");
  // Spec 0102: start path of this session (`"."` = home), target of "Go to
  // start directory". Comes once from the backend (`sftp_start_directory`).
  const [startPath, setStartPath] = useState(".");
  const [entries, setEntries] = useState<RemoteEntryDto[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // `elevatedOverride`: beim Umschalten ist der neue Modus im State noch
  // nicht angekommen — der Aufrufer übergibt ihn dann explizit.
  // Issue #624: the mode of every listed entry, so an empty listing of a
  // folder can be explained by the folder's own (missing) execute bit.
  const knownDirModes = useRef(new Map<string, number>());

  const load = useCallback(
    (targetPath: string, userOverride?: string | null) => {
      setLoading(true);
      setError(null);
      sftpList(
        sessionId,
        targetPath,
        userOverride === undefined ? channelUser : userOverride,
      )
        .then((result) => {
          for (const e of result)
            knownDirModes.current.set(e.path, e.permissionsOctal);
          setEntries(result);
          setPath(targetPath);
          setPathInput(targetPath);
        })
        .catch((err) => setError(commandErrorMessage(err)))
        .finally(() => setLoading(false));
      // eslint-disable-next-line react-hooks/exhaustive-deps -- load reads state through refs; identity is irrelevant
    },
    [sessionId, channelUser],
  );

  useEffect(() => {
    // Spec 0102: erst den Startpfad erfragen (ein fehlendes oder nicht
    // erreichbares Startverzeichnis liefert `"."` und ggf. den einmaligen
    // Hinweis); scheitert schon die Abfrage, wie bisher im Home starten.
    let cancelled = false;
    const queryStart = async () => {
      try {
        return await sftpStartDirectory(sessionId);
      } catch {
        return null;
      }
    };
    void queryStart().then((start) => {
      if (cancelled) return;
      const target = start?.path ?? ".";
      notifyMissingStartDirectory(t, start?.missingDirectory ?? null);
      setStartPath(target);
      load(target);
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- load once per session
  }, [sessionId]);

  const pathRef = useRef(path);
  pathRef.current = path;
  /** Issue #355: reloads the current directory on the normal channel (home if it is gone). */
  const reloadAfterReconnect = () => {
    setLoading(true);
    setError(null);
    const show = (target: string) => (result: RemoteEntryDto[]) => {
      setEntries(result);
      setPath(target);
      setPathInput(target);
    };
    const previous = pathRef.current;
    sftpList(sessionId, previous, null)
      .then(show(previous), () =>
        sftpList(sessionId, ".", null).then(show(".")),
      )
      .catch((err) => setError(commandErrorMessage(err)))
      .finally(() => setLoading(false));
  };

  return {
    path,
    pathRef,
    pathInput,
    setPathInput,
    startPath,
    entries,
    loading,
    error,
    setError,
    knownDirModes,
    load,
    reloadAfterReconnect,
  };
}
