import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

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

/**
 * Spec 0076, B-2: der Dateidialog für den Pfad einer Schlüsseldatei —
 * anders als [`pickAndReadTextFile`] geht es hier **um den Pfad selbst**,
 * nicht um Dateiinhalt (`AuthMethod::IdentityFile { path }` speichert den
 * Pfad, wie der Nutzer ihn angibt, s. Spec 0076 A-1/§4.4). Der Aufruf bleibt
 * deshalb im Webview, wie bei anderen reinen Pfad-Auswahlen dieser App
 * (`FileTypeSettings.tsx`, `FileBrowserPanel.tsx`) — kein Backend-Umweg
 * nötig, weil hier nie Dateiinhalt zurückkommt, an dem sich die
 * `read_credential_file`-Begründung (Pfad bleibt beim Backend) festmachen
 * würde. `null`, falls der Dialog abgebrochen wurde.
 */
export async function pickFilePath(title: string): Promise<string | null> {
  const picked = await open({ title, multiple: false, directory: false });
  return typeof picked === "string" ? picked : null;
}
