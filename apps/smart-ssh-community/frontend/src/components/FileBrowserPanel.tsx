import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import {
  commandErrorMessage,
  sftpDelete,
  sftpDownload,
  sftpList,
  sftpMkdir,
  sftpRename,
  sftpUpload,
} from "../api";
import { onSftpTransferFinished, onSftpTransferStarted } from "../events";
import { formatBytes } from "../format";
import {
  loadFileManagerColumnWidths,
  saveFileManagerColumnWidths,
  type FileManagerColumnWidths,
} from "../layoutSettings";
import { displayPath, joinPath, localBaseName, parentPath } from "../remotePath";
import type { RemoteEntryDto } from "../types";
import { useDragResize } from "../useDragResize";

/** Spec 0053, Teil 1: Standard-/Mindestbreiten der verstellbaren Spalten
 * (alles in px). Die Name-Spalte hat bewusst keinen eigenen Eintrag hier
 * — sie bekommt nie eine explizite Breite (s. `<colgroup>` unten), nimmt
 * unter `table-layout: fixed` also automatisch den nach den anderen
 * Spalten verbleibenden Rest ein. `NAME_MIN_WIDTH` fließt stattdessen in
 * `clampColumnWidth` ein, damit eine sehr breit gezogene Nachbarspalte
 * die Name-Spalte nie unter ihre eigene Mindestbreite drückt. */
const DEFAULT_COLUMN_WIDTHS: FileManagerColumnWidths = {
  size: 90,
  permissions: 90,
  modified: 150,
};
const MIN_COLUMN_WIDTHS: FileManagerColumnWidths = {
  size: 56,
  permissions: 64,
  modified: 90,
};
const NAME_MIN_WIDTH = 120;
/** Feste, nicht verstellbare Aktionsspalte (⋮-Menü) — kein Label, immer
 * gleich schmal, kein Drag-Handle. */
const ACTIONS_COLUMN_WIDTH = 36;

interface Transfer {
  id: string;
  kind: "upload" | "download";
  fileName: string;
  totalBytes: number | null;
  error: string | null;
}

interface FileBrowserPanelProps {
  sessionId: string;
  /** Spec 0020, Abschnitt 5.4/Spec 0017: nur sichtbar (und damit für
   * Drag-and-Drop aktiv), wenn dies der aktive Tab UND die "Dateien"-Ansicht
   * gerade gewählt ist — bleibt sonst wie `TerminalView` gemountet (eigener
   * Navigationszustand pro Tab bleibt erhalten), nur per CSS ausgeblendet. */
  isVisible: boolean;
}

/**
 * Manueller SFTP-Dateibrowser (Spec 0020, Abschnitt 5). Läuft komplett ohne
 * Filter-Engine-Prüfung — direkte Nutzeraktionen, wie im interaktiven
 * Terminal. Eigener, pro Tab getrennter Navigationszustand (Abschnitt 5.4):
 * diese Komponente wird einmal pro Session-Tab instanziiert und bleibt beim
 * Tab-Wechsel gemountet (s. `SessionView`), ihr `path`-State lebt daher
 * automatisch pro Tab getrennt, ohne zusätzliche Buchführung.
 */
export function FileBrowserPanel({ sessionId, isVisible }: FileBrowserPanelProps) {
  const [path, setPath] = useState(".");
  const [pathInput, setPathInput] = useState(".");
  const [entries, setEntries] = useState<RemoteEntryDto[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<{ entry: RemoteEntryDto; value: string } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<RemoteEntryDto | null>(null);
  const [mkdirOpen, setMkdirOpen] = useState<string | null>(null);
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const [dragOver, setDragOver] = useState(false);
  const [columnWidths, setColumnWidths] = useState<FileManagerColumnWidths>(DEFAULT_COLUMN_WIDTHS);
  const tableContainerRef = useRef<HTMLDivElement>(null);

  // Spec 0053, Teil 1: einmalig beim Mounten geladen (diese Komponente lebt
  // pro Tab, s. Doc-Kommentar oben) — ungültige/fehlende Felder bleiben
  // beim eingebauten Default (`loadFileManagerColumnWidths` liefert nur
  // die tatsächlich gültigen Felder zurück, s. dortiger Kommentar).
  useEffect(() => {
    loadFileManagerColumnWidths()
      .then((stored) => setColumnWidths((prev) => ({ ...prev, ...stored })))
      .catch((err) => console.warn("Konnte Spaltenbreiten nicht laden:", err));
  }, []);

  /** Klemmt eine vorgeschlagene Breite für `column` auf ihre Mindestbreite
   * UND darauf, dass die Name-Spalte (die den Rest der verfügbaren Breite
   * einnimmt, s. `<colgroup>` unten) nie unter `NAME_MIN_WIDTH` fällt —
   * abhängig von der aktuell tatsächlich verfügbaren Breite des
   * Tabellen-Containers, nicht von einem festen Annahmewert. */
  const clampColumnWidth = useCallback(
    (column: keyof FileManagerColumnWidths, proposed: number, widths: FileManagerColumnWidths) => {
      const min = MIN_COLUMN_WIDTHS[column];
      const containerWidth = tableContainerRef.current?.clientWidth ?? Infinity;
      const otherColumnsTotal =
        ACTIONS_COLUMN_WIDTH +
        (Object.keys(widths) as (keyof FileManagerColumnWidths)[])
          .filter((key) => key !== column)
          .reduce((sum, key) => sum + widths[key], 0);
      const maxForThisColumn = containerWidth - otherColumnsTotal - NAME_MIN_WIDTH;
      return Math.min(Math.max(proposed, min), Math.max(min, maxForThisColumn));
    },
    [],
  );

  // Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts): der
  // Persistenz-Aufruf lief vorher als Seiteneffekt *innerhalb* eines
  // `setState`-Updaters (`setColumnWidths((current) => { save(current);
  // return current; })`) — unter `StrictMode` (aktiv in `main.tsx`) ruft
  // React Updater im Dev-Modus doppelt auf, was `store.save()` doppelt
  // ausgelöst hätte (harmlos, da idempotent, aber ein vermeidbares
  // Anti-Pattern). `columnWidthsRef` hält den aktuellen Wert stattdessen
  // synchron zum State nach, `handleColumnDragEnd` liest ihn direkt statt
  // über einen Updater.
  const columnWidthsRef = useRef(columnWidths);
  useEffect(() => {
    columnWidthsRef.current = columnWidths;
  }, [columnWidths]);

  const handleColumnDrag = useCallback(
    (column: keyof FileManagerColumnWidths) => (deltaX: number) => {
      setColumnWidths((prev) => {
        const clamped = clampColumnWidth(column, prev[column] + deltaX, prev);
        if (clamped === prev[column]) return prev;
        return { ...prev, [column]: clamped };
      });
    },
    [clampColumnWidth],
  );

  const handleColumnDragEnd = useCallback(() => {
    saveFileManagerColumnWidths(columnWidthsRef.current).catch((err) =>
      console.warn("Konnte Spaltenbreiten nicht speichern:", err),
    );
  }, []);

  const load = useCallback((targetPath: string) => {
    setLoading(true);
    setError(null);
    sftpList(sessionId, targetPath)
      .then((result) => {
        setEntries(result);
        setPath(targetPath);
        setPathInput(targetPath);
      })
      .catch((err) => setError(commandErrorMessage(err)))
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  useEffect(() => {
    load(".");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  // Schließt das Kontextmenü bei einem Klick irgendwo sonst hin.
  useEffect(() => {
    if (menuFor === null) return;
    const handler = () => setMenuFor(null);
    document.addEventListener("click", handler);
    return () => document.removeEventListener("click", handler);
  }, [menuFor]);

  useEffect(() => {
    const unlisten = [
      onSftpTransferStarted((event) => {
        if (event.sessionId !== sessionId) return;
        setTransfers((prev) => [
          ...prev,
          {
            id: event.transferId,
            kind: event.kind,
            fileName: event.fileName,
            totalBytes: event.totalBytes,
            error: null,
          },
        ]);
      }),
      onSftpTransferFinished((event) => {
        if (event.sessionId !== sessionId) return;
        if (event.error) {
          setTransfers((prev) =>
            prev.map((t) => (t.id === event.transferId ? { ...t, error: event.error } : t)),
          );
        } else {
          setTransfers((prev) => prev.filter((t) => t.id !== event.transferId));
          // Erfolgreicher Transfer kann den Inhalt des aktuell offenen
          // Verzeichnisses geändert haben (Upload/Download berührt aber nur
          // die Ziel-/Quelldatei — ein Refresh ist trotzdem billig genug,
          // um nicht auf mögliche Rennbedingungen zwischen Event und
          // Nutzer-Navigation Rücksicht nehmen zu müssen).
          load(path);
        }
      }),
    ];
    return () => {
      unlisten.forEach((p) => p.then((fn) => fn()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, path]);

  // Spec 0020, Abschnitt 5.1: Upload per Drag-and-Drop aus dem Betriebssystem.
  // Nur aktiv, während dieser Tab UND die Dateien-Ansicht sichtbar sind (s.
  // `isVisible`-Doc-Kommentar oben) — `onDragDropEvent` ist global für das
  // gesamte Fenster, ohne dieses Gate würden gleichzeitig mehrere
  // (unsichtbar) gemountete Dateibrowser-Instanzen anderer Tabs denselben
  // Drop ebenfalls als Upload in ihr jeweiliges Verzeichnis interpretieren.
  useEffect(() => {
    if (!isVisible) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "over") {
          setDragOver(true);
        } else if (event.payload.type === "leave") {
          setDragOver(false);
        } else if (event.payload.type === "drop") {
          setDragOver(false);
          for (const localPath of event.payload.paths) {
            startUpload(localPath, joinPath(path, localBaseName(localPath)));
          }
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isVisible, path]);

  const startUpload = (localPath: string, remotePath: string) => {
    sftpUpload(sessionId, localPath, remotePath).catch((err) => {
      setError(commandErrorMessage(err));
    });
  };

  const handleUploadButton = async () => {
    const picked = await open({ title: "Datei(en) hochladen", multiple: true, directory: false });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    for (const localPath of paths) {
      startUpload(localPath, joinPath(path, localBaseName(localPath)));
    }
  };

  const handleDownload = (entry: RemoteEntryDto) => {
    setMenuFor(null);
    sftpDownload(sessionId, entry.path).catch((err) => setError(commandErrorMessage(err)));
  };

  const handleConfirmDelete = () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setDeleteTarget(null);
    sftpDelete(sessionId, target.path)
      .then(() => load(path))
      .catch((err) => setError(commandErrorMessage(err)));
  };

  const handleConfirmRename = () => {
    if (!renaming) return;
    const trimmed = renaming.value.trim();
    if (!trimmed || trimmed === renaming.entry.name) {
      setRenaming(null);
      return;
    }
    const newPath = joinPath(path, trimmed);
    sftpRename(sessionId, renaming.entry.path, newPath)
      .then(() => load(path))
      .catch((err) => setError(commandErrorMessage(err)))
      .finally(() => setRenaming(null));
  };

  const handleConfirmMkdir = () => {
    const name = mkdirOpen?.trim();
    setMkdirOpen(null);
    if (!name) return;
    sftpMkdir(sessionId, joinPath(path, name))
      .then(() => load(path))
      .catch((err) => setError(commandErrorMessage(err)));
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-1.5 border-b border-slate-800 px-2 py-1.5">
        <button
          type="button"
          onClick={() => load(".")}
          title="Zum Startverzeichnis"
          className="border border-slate-700 px-2 py-1 text-xs text-slate-300 hover:bg-slate-800"
        >
          ⌂
        </button>
        <button
          type="button"
          onClick={() => load(parentPath(path))}
          disabled={path === "."}
          title="Übergeordnetes Verzeichnis"
          className="border border-slate-700 px-2 py-1 text-xs text-slate-300 hover:bg-slate-800 disabled:opacity-40"
        >
          ↑
        </button>
        <form
          className="min-w-0 flex-1"
          onSubmit={(e) => {
            e.preventDefault();
            load(pathInput.trim() || ".");
          }}
        >
          <input
            value={pathInput}
            onChange={(e) => setPathInput(e.target.value)}
            className="w-full border border-slate-700 bg-slate-950 px-2 py-1 font-mono text-xs text-slate-200 focus:outline-none"
          />
        </form>
        <button
          type="button"
          onClick={() => setMkdirOpen("")}
          className="border border-slate-700 px-2 py-1 text-xs text-slate-300 hover:bg-slate-800"
        >
          + Ordner
        </button>
        <button
          type="button"
          onClick={handleUploadButton}
          className="font-heading border border-indigo-600/50 px-2 py-1 text-xs font-semibold text-indigo-400 hover:bg-indigo-600/14"
        >
          Hochladen
        </button>
      </div>

      {transfers.length > 0 && (
        <div className="space-y-1 border-b border-slate-800 bg-slate-950/60 px-2 py-1.5">
          {transfers.map((t) => (
            <div key={t.id} className="flex items-center gap-2 text-xs">
              {t.error ? (
                <>
                  <span className="text-red-400">✗</span>
                  <span className="min-w-0 flex-1 truncate text-red-300">
                    {t.fileName}: {t.error}
                  </span>
                  <button
                    type="button"
                    onClick={() => setTransfers((prev) => prev.filter((x) => x.id !== t.id))}
                    className="text-red-400 hover:text-red-300"
                  >
                    ✕
                  </button>
                </>
              ) : (
                <>
                  <span className="inline-block h-2 w-2 shrink-0 animate-pulse rounded-full bg-indigo-400" />
                  <span className="min-w-0 flex-1 truncate text-slate-300">
                    {t.kind === "upload" ? "Hochladen" : "Herunterladen"}: {t.fileName}
                    {t.totalBytes !== null && ` (${formatBytes(t.totalBytes)})`}
                  </span>
                </>
              )}
            </div>
          ))}
        </div>
      )}

      <div
        ref={tableContainerRef}
        className={`relative min-h-0 flex-1 overflow-auto ${dragOver ? "bg-indigo-950/40" : ""}`}
      >
        {loading && <p className="p-3 text-xs text-slate-400">Lädt…</p>}
        {error && <p className="p-3 text-xs text-red-400">{error}</p>}
        {!loading && !error && entries.length === 0 && (
          <p className="p-3 text-xs text-slate-500">(leeres Verzeichnis)</p>
        )}
        {!loading && !error && entries.length > 0 && (
          <table
            className="w-full table-fixed text-left text-xs"
            style={{
              // Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts):
              // ohne diese `min-width` konnte die Name-Spalte unter
              // `table-fixed` auf tatsächlich 0px kollabieren, sobald der
              // SSH-/SFTP-Bereich (Teil 2, per Divider verstellbar) schmaler
              // war als die Summe der anderen Spalten — Dateinamen
              // verschwanden komplett statt nur abgeschnitten zu werden,
              // und `clampColumnWidth`s `NAME_MIN_WIDTH` griff dabei gar
              // nicht (das klemmt nur beim Spalten-Drag selbst, nicht beim
              // Schmaler-Werden des ganzen Panels). Mit dieser Mindestbreite
              // wächst die Tabelle stattdessen über den Container hinaus,
              // und der umgebende `overflow-auto`-Container (s. oben)
              // scrollt horizontal statt die Name-Spalte zu verschlucken.
              minWidth:
                columnWidths.size +
                columnWidths.permissions +
                columnWidths.modified +
                ACTIONS_COLUMN_WIDTH +
                NAME_MIN_WIDTH,
            }}
          >
            {/* `table-fixed` + `<colgroup>` statt des vorherigen
                automatischen Spaltenlayouts — nur so lassen sich
                Spaltenbreiten gezielt per Drag setzen. Die Name-Spalte hat
                bewusst keine `width` (kein `<col>`-Attribut dafür) — unter
                `table-fixed` nimmt eine Spalte ohne explizite Breite den
                nach den anderen verbleibenden Rest ein, genau das von der
                Spec verlangte "flexibler Rest"-Verhalten. */}
            <colgroup>
              <col />
              <col style={{ width: columnWidths.size }} />
              <col style={{ width: columnWidths.permissions }} />
              <col style={{ width: columnWidths.modified }} />
              <col style={{ width: ACTIONS_COLUMN_WIDTH }} />
            </colgroup>
            <thead className="sticky top-0 bg-slate-900 text-slate-500">
              <tr className="[&>th]:px-2 [&>th]:py-1 [&>th]:font-normal">
                <th>Name</th>
                <th className="relative">
                  Größe
                  <ColumnResizeHandle
                    testId="column-resize-size"
                    onDrag={handleColumnDrag("size")}
                    onDragEnd={handleColumnDragEnd}
                  />
                </th>
                <th className="relative">
                  Rechte
                  <ColumnResizeHandle
                    testId="column-resize-permissions"
                    onDrag={handleColumnDrag("permissions")}
                    onDragEnd={handleColumnDragEnd}
                  />
                </th>
                <th className="relative">
                  Geändert
                  <ColumnResizeHandle
                    testId="column-resize-modified"
                    onDrag={handleColumnDrag("modified")}
                    onDragEnd={handleColumnDragEnd}
                  />
                </th>
                <th />
              </tr>
            </thead>
            <tbody>
              {entries.map((entry) => (
                <tr key={entry.path} className="border-t border-slate-800/70 hover:bg-slate-800/40">
                  <td className="max-w-0 px-2 py-1">
                    {entry.isDir ? (
                      <button
                        type="button"
                        onClick={() => load(entry.path)}
                        className="truncate text-left text-indigo-300 hover:underline"
                        title={entry.name}
                      >
                        📁 {entry.name}
                      </button>
                    ) : (
                      <span className="block truncate text-slate-200" title={entry.name}>
                        📄 {entry.name}
                      </span>
                    )}
                  </td>
                  <td className="px-2 py-1 whitespace-nowrap text-slate-400">
                    {entry.isDir ? "—" : formatBytes(entry.size)}
                  </td>
                  <td className="px-2 py-1 whitespace-nowrap font-mono text-slate-500">
                    {entry.permissions}
                  </td>
                  <td className="px-2 py-1 whitespace-nowrap text-slate-500">
                    {entry.modified ? new Date(entry.modified).toLocaleString() : "—"}
                  </td>
                  <td className="relative px-2 py-1 text-right">
                    <button
                      type="button"
                      onClick={() => setMenuFor(menuFor === entry.path ? null : entry.path)}
                      className="px-1.5 text-slate-400 hover:text-slate-100"
                    >
                      ⋮
                    </button>
                    {menuFor === entry.path && (
                      <div className="absolute right-2 top-full z-10 w-40 border border-slate-700 bg-slate-900 py-1 text-left shadow-lg">
                        {!entry.isDir && (
                          <button
                            type="button"
                            onClick={() => handleDownload(entry)}
                            className="block w-full px-3 py-1.5 text-left text-slate-200 hover:bg-indigo-600/14"
                          >
                            Herunterladen
                          </button>
                        )}
                        <button
                          type="button"
                          onClick={() => {
                            setMenuFor(null);
                            setRenaming({ entry, value: entry.name });
                          }}
                          className="block w-full px-3 py-1.5 text-left text-slate-200 hover:bg-indigo-600/14"
                        >
                          Umbenennen
                        </button>
                        {!entry.isDir && (
                          <button
                            type="button"
                            onClick={() => {
                              setMenuFor(null);
                              setDeleteTarget(entry);
                            }}
                            className="block w-full px-3 py-1.5 text-left text-red-400 hover:bg-red-600/12"
                          >
                            Löschen
                          </button>
                        )}
                      </div>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {dragOver && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center border-2 border-dashed border-indigo-500 bg-indigo-950/30 text-sm text-indigo-200">
            Hier ablegen zum Hochladen nach {displayPath(path)}
          </div>
        )}
      </div>

      {renaming && (
        <RenamePrompt
          initialValue={renaming.value}
          onChange={(value) => setRenaming({ entry: renaming.entry, value })}
          onCancel={() => setRenaming(null)}
          onConfirm={handleConfirmRename}
        />
      )}

      {mkdirOpen !== null && (
        <RenamePrompt
          title="Neuer Ordner"
          initialValue={mkdirOpen}
          onChange={setMkdirOpen}
          onCancel={() => setMkdirOpen(null)}
          onConfirm={handleConfirmMkdir}
        />
      )}

      {deleteTarget && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
          <div className="w-full max-w-sm border border-red-700/50 bg-slate-900 p-5 shadow-xl">
            <h2 className="font-heading mb-2 text-sm font-semibold text-red-300">
              Datei löschen?
            </h2>
            <p className="mb-4 text-sm text-slate-300">
              <span className="font-mono text-xs break-all">{deleteTarget.path}</span> wird
              unwiderruflich vom Server gelöscht.
            </p>
            <div className="flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setDeleteTarget(null)}
                className="font-heading border border-slate-600 px-3 py-1.5 text-xs font-semibold text-slate-200 hover:bg-slate-800"
              >
                Abbrechen
              </button>
              <button
                type="button"
                onClick={handleConfirmDelete}
                className="font-heading bg-red-600 px-3 py-1.5 text-xs font-semibold text-red-50 hover:bg-red-500"
              >
                Löschen
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/** Gemeinsame kleine Eingabeaufforderung für "Umbenennen" und "Neuer
 * Ordner" — beide brauchen nur einen einzelnen Namen mit Bestätigen/
 * Abbrechen, ein voller Modal-Dialog (wie beim Löschen) wäre hier
 * überdimensioniert. */
function RenamePrompt({
  title = "Umbenennen",
  initialValue,
  onChange,
  onCancel,
  onConfirm,
}: {
  title?: string;
  initialValue: string;
  onChange: (value: string) => void;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
      <div className="w-full max-w-sm border border-slate-700 bg-slate-900 p-5 shadow-xl">
        <h2 className="font-heading mb-2 text-sm font-semibold text-slate-100">{title}</h2>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            onConfirm();
          }}
        >
          <input
            ref={inputRef}
            value={initialValue}
            onChange={(e) => onChange(e.target.value)}
            className="mb-4 w-full border border-slate-600 bg-slate-950 px-2 py-1.5 font-mono text-sm text-slate-100 focus:outline-none"
          />
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={onCancel}
              className="font-heading border border-slate-600 px-3 py-1.5 text-xs font-semibold text-slate-200 hover:bg-slate-800"
            >
              Abbrechen
            </button>
            <button
              type="submit"
              className="font-heading bg-indigo-600 px-3 py-1.5 text-xs font-semibold text-slate-950 hover:bg-indigo-500"
            >
              Übernehmen
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

/** Spec 0053, Teil 1: Drag-Handle am rechten Rand eines Spaltenkopfs — die
 * Trefferfläche (`w-2`, per `-right-1` über die Spaltengrenze zentriert)
 * ist bewusst breiter als die sichtbare Trennlinie (`w-px`), damit sie
 * auch ohne Pixel-genaues Zielen zu treffen ist. `cursor-col-resize` /
 * `touch-none` (kein Scroll-Gestenkonflikt beim Ziehen) als der von der
 * Spec verlangte Hover-/Cursor-Hinweis.
 *
 * Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts): kein
 * `role="separator"`/`aria-label` mehr — ein per Tastatur nicht
 * bedienbares, rein visuelles Maus-Element (die Spec verlangt
 * Tastaturbedienbarkeit ausdrücklich nicht) würde Screenreadern beim
 * Durchlaufen der Tabellenkopfzeile als benanntes, aber funktionsloses
 * Element angesagt — irreführend. `aria-hidden` ist hier die ehrlichere
 * Wahl; `data-testid` bleibt als reiner Test-Anker. */
function ColumnResizeHandle({
  testId,
  onDrag,
  onDragEnd,
}: {
  testId: string;
  onDrag: (deltaX: number) => void;
  onDragEnd: () => void;
}) {
  const handlers = useDragResize(onDrag, onDragEnd);
  return (
    <span
      aria-hidden="true"
      data-testid={testId}
      className="group absolute -right-1 top-0 bottom-0 z-10 flex w-2 cursor-col-resize touch-none select-none justify-center"
      {...handlers}
    >
      <span className="h-full w-px bg-slate-700 group-hover:bg-indigo-500" />
    </span>
  );
}
