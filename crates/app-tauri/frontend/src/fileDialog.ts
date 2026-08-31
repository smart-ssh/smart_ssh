import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

/**
 * Öffnet den nativen Datei-Dialog, liest die gewählte Datei über den
 * Backend-Command `read_credential_file` (Spec 0013, SEC-06). Nur der
 * Inhalt wird zurückgegeben, der Pfad selbst wird nirgends gespeichert.
 */
export async function pickAndReadTextFile(title: string): Promise<string | null> {
  const path = await open({ title, multiple: false, directory: false });
  if (!path || Array.isArray(path)) return null;
  return invoke<string>("read_credential_file", { path });
}
