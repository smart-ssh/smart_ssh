import { invoke } from "@tauri-apps/api/core";

/**
 * Ruft den Backend-Command `read_credential_file` (Spec 0013, SEC-06) auf,
 * der den nativen Datei-Dialog SELBST öffnet und nur den Inhalt der
 * gewählten Datei zurückliefert — nie einen Pfad. Unabhängiger
 * Review-Pass: der Dialog lief zuvor im Webview
 * (`@tauri-apps/plugin-dialog`s `open()`), das den gewählten Pfad
 * anschließend an `read_credential_file` weiterreichte; jeder andere Code
 * im Webview hätte denselben Command genauso gut mit einem
 * selbstgewählten Pfad aufrufen können (`invoke("read_credential_file",
 * { path: "~/.ssh/id_rsa" })`), da das Backend den Pfad ungeprüft las.
 * Nur der Anzeige-`title` geht noch ans Backend, kein Pfad — nur eine
 * tatsächliche Interaktion mit dem nativen Dialog liefert einen Pfad.
 * `null`, falls der Dialog abgebrochen wurde.
 */
export async function pickAndReadTextFile(title: string): Promise<string | null> {
  return invoke<string | null>("read_credential_file", { title });
}
