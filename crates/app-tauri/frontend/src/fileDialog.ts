import { open } from "@tauri-apps/plugin-dialog";
import { readTextFile } from "@tauri-apps/plugin-fs";

/**
 * Öffnet den nativen Datei-Dialog, liest die gewählte Datei clientseitig
 * (Spec 0008, Abschnitt 6: "Key-Datei-Auswahl via Tauri-Datei-Dialog,
 * liest die Datei client- oder serverseitig ein — deine Wahl"). Nur der
 * Inhalt wird zurückgegeben, der Pfad selbst wird nirgends gespeichert
 * (Spec Abschnitt 8: "der ursprüngliche Dateipfad wird nicht
 * gespeichert").
 */
export async function pickAndReadTextFile(title: string): Promise<string | null> {
  const path = await open({ title, multiple: false, directory: false });
  if (!path || Array.isArray(path)) return null;
  return readTextFile(path);
}
