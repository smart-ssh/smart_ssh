/** Menschenlesbare Byte-Größe (`B`/`KB`/`MB`), gemeinsam genutzt von der
 * Binärdatei-Änderungsanzeige (Spec 0020, Abschnitt 4.2, `ChatPanel`) und
 * dem manuellen Dateibrowser (Spec 0020, Abschnitt 5, `FileBrowserPanel`). */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
