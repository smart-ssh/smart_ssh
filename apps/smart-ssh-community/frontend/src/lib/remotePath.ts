// Spec 0020, Abschnitt 5: reine Pfad-Hilfsfunktionen für den manuellen
// Dateibrowser. `SftpSession::list_dir`/`stat` (s. `crate::ssh::sftp`)
// liefern Pfade unverändert wie vom `russh-sftp`-Client zusammengesetzt —
// bei einem relativen Startpfad (".") bleiben auch alle Kindpfade relativ
// ("./unterordner", ohne Kanonisierung; der Trait bietet kein `realpath`).
// Diese Funktionen operieren rein auf dem String, ohne Annahmen über
// absolut/relativ zu treffen — funktioniert für beide Formen gleichermaßen.

/** Übergeordneter Pfad, für den "Aufwärts"-Button. `"."` (Startverzeichnis)
 * bleibt bei sich selbst — es gibt kein "darüber" ohne `realpath`. */
export function parentPath(path: string): string {
  if (path === "." || path === "" || path === "/") return ".";
  const idx = path.lastIndexOf("/");
  if (idx < 0) return ".";
  if (idx === 0) return "/";
  return path.slice(0, idx);
}

/** Verbindet ein Verzeichnis mit einem Eintragsnamen — für neu angelegte
 * Ziele (Upload, "Neuer Ordner"), deren Pfad die App selbst bilden muss
 * (anders als Einträge aus `list_dir`, deren `path` bereits vom Server
 * kommt). */
export function joinPath(dir: string, name: string): string {
  if (dir === "." || dir === "") return name;
  return dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`;
}

/** Nur für die Anzeige in der Pfadleiste — `"."` liest sich als "Start"-Pfad
 * klarer denn als bloßer Punkt. */
export function displayPath(path: string): string {
  return path === "." ? "~" : path;
}

/** Letztes Pfadsegment eines LOKALEN Pfads (Upload-Quelle) — funktioniert für
 * sowohl POSIX- (`/`) als auch Windows-Pfade (`\`), da der native
 * Datei-Dialog bzw. ein OS-Drop-Ereignis je nach Plattform beide Formen
 * liefern kann. */
export function localBaseName(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const segments = normalized.split("/").filter((s) => s.length > 0);
  return segments[segments.length - 1] ?? path;
}
