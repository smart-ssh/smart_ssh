import { useTranslation } from "react-i18next";
import { formatBytes } from "../format";
import { isDiffTooLargeToCompute, shortNoteDiff } from "../textDiff";

/**
 * Spec 0019, Abschnitt 4: kurze Änderungs-Vorschau für einen
 * `ProposeNoteUpdate`-Vorschlag — nur die tatsächlich geänderten Zeilen
 * (hinzugefügt grün, entfernt rot/durchgestrichen), nicht der volle
 * bestehende Notiztext. Gemeinsam genutzt von `ChatPanel` (In-Chat-
 * Vorschlag) und `NoteSuggestionToast` (Disconnect-Vorschlag, Spec 0010) —
 * beide sollen dieselbe Darstellung zeigen.
 */
export function NoteDiffPreview({
  previousContent,
  newContent,
}: {
  previousContent: string | null;
  newContent: string;
}) {
  const { t } = useTranslation();
  const before = previousContent ?? "";

  // Spec 0046, Fund 3: über dem Cap keinen O(n·m)-Zeilen-Diff berechnen
  // (Confirm-Dialog-Renderer-Einfrierer bei großen Dateien, s.
  // `isDiffTooLargeToCompute`-Doc-Kommentar) — Hinweis + Größenangabe statt
  // Diff, analog zum Binärdatei-Fall (`BinaryFileChangeHint`). Der
  // Schreibvorgang selbst ist davon unberührt, nur diese Vorschau ist
  // gekürzt.
  if (isDiffTooLargeToCompute(before, newContent)) {
    const byteLength = (s: string) => new TextEncoder().encode(s).length;
    return (
      <p className="border border-slate-700 bg-slate-950 px-2 py-1.5 text-xs text-slate-400">
        {t("confirmDialog.diffTooLargeHint", {
          oldSize: formatBytes(byteLength(before)),
          newSize: formatBytes(byteLength(newContent)),
        })}
      </p>
    );
  }

  const lines = shortNoteDiff(previousContent, newContent);

  if (lines.length === 0) {
    return <p className="text-xs text-slate-500">{t("confirmDialog.noContentChange")}</p>;
  }

  return (
    <div className="space-y-0.5 border border-slate-700 bg-slate-950 px-2 py-1.5 font-mono text-xs">
      {lines.map((line, i) => (
        <div
          key={i}
          className={
            line.type === "added"
              ? "text-emerald-400"
              : "text-red-400 line-through decoration-red-500/60"
          }
        >
          <span className="select-none opacity-70">{line.type === "added" ? "+ " : "− "}</span>
          {line.text || " "}
        </div>
      ))}
    </div>
  );
}
