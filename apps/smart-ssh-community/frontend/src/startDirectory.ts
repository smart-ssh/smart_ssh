// Spec 0102: Prüfung des optionalen Startverzeichnisses im Server-Formular —
// dieselben Regeln wie `normalize_start_directory` im Backend
// (`crates/core/src/profiles/start_directory.rs`), damit ein ungültiger Wert
// schon vor dem Speichern eine klare Meldung bekommt.

import type { TFunction } from "i18next";
import { showToast } from "./toastBus";

export type StartDirectoryCheck =
  | { ok: true; value: string | null }
  | { ok: false; reason: "notAbsolute" | "controlCharacter" };

// eslint-disable-next-line no-control-regex
const CONTROL_CHARACTER = /[\u0000-\u001f\u007f-\u009f]/;

/** Randleerraum weg; leer = nicht gesetzt (`null`). Erlaubt sind `/…` und
 * `~/…`; `~` allein, `~nutzer/…` und andere relative Pfade nicht. */
export function checkStartDirectory(input: string): StartDirectoryCheck {
  const trimmed = input.trim();
  if (trimmed === "") return { ok: true, value: null };
  if (CONTROL_CHARACTER.test(trimmed)) return { ok: false, reason: "controlCharacter" };
  if (!trimmed.startsWith("/") && !trimmed.startsWith("~/")) {
    return { ok: false, reason: "notAbsolute" };
  }
  return { ok: true, value: trimmed };
}

/** Spec 0102: der sichtbare, nicht blockierende Hinweis auf ein fehlendes
 * Startverzeichnis. Das Backend liefert den Wert pro Sitzung genau einmal
 * (an Terminal oder Dateibrowser, wer zuerst fragt), daher kein eigenes
 * Entprellen hier. */
export function notifyMissingStartDirectory(t: TFunction, missingDirectory: string | null): void {
  if (!missingDirectory) return;
  showToast({ kind: "error", message: t("startDirectory.notFound", { dir: missingDirectory }) });
}
