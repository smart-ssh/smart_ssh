import { TakeIntoNoteButton } from "./ActionCard";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { DocumentFormat } from "../../types";
import { Button } from "../ui";
import { MARKDOWN_COMPONENTS } from "./DocumentCard";

/** Spec 0055, Teil 2: "Export to Markdown"/"Als Notiz übernehmen" hingen
 * bisher an JEDER Antwort, auch an trivialen ("ok, verstanden") —
 * verwässert die Bedeutung der Aktionen und wirkt unaufgeräumt. Gewählter
 * Weg: dezent statt permanent — die Aktionsleiste blendet erst bei Hover
 * (bzw. Tastaturfokus auf einem ihrer Buttons, `focus-within` — sonst wäre
 * sie für reine Tastaturbedienung unerreichbar) ein, statt dauerhaft
 * sichtbar zu sein (die dritte von der Spec vorgeschlagene Unterscheidung,
 * "Dokument vs. normale Antwort", besteht strukturell bereits:
 * `AssistantMessageView` ist von `DocumentCard` getrennt, die Aktionen
 * hier erscheinen nie an einem generierten Dokument, s. dortige
 * Komponente ohne `TakeIntoNoteButton`).
 *
 * **Bewusst NICHT zusätzlich unterhalb einer Mindestlänge ganz entfernt**
 * (Spec-Reviewer-Fund, Spec 0055, Review des Gesamtpakets): eine frühere
 * Fassung blendete die Aktionen unter ~40 Zeichen komplett aus dem DOM
 * aus — das verletzte die Spec wörtlich ("Keine bestehende Funktion
 * entfernen — nur die Darstellung/Platzierung verbessern"), denn eine
 * kurze, aber notizwürdige Antwort (z. B. "rm -rf / löscht das ganze
 * System") wäre dann für "In Notiz übernehmen" gar nicht mehr erreichbar
 * gewesen, auch nicht per Tastatur. Hover/Fokus allein löst das
 * ursprüngliche Problem (dauerhafte Sichtbarkeit an trivialen Antworten)
 * bereits vollständig, ohne diese Erreichbarkeits-Lücke. */
export function AssistantMessageView({
  text,
  onExport,
  sessionId,
  truncated,
  onContinue,
}: {
  text: string;
  onExport: (contentMarkdown: string, title: string, format: DocumentFormat) => Promise<void>;
  sessionId: string;
  /** Spec 0065, Teil 2 — s. `ChatItem`s `truncated`-Feld-Kommentar. */
  truncated: boolean;
  onContinue: () => void;
}) {
  const { t } = useTranslation();
  const [exporting, setExporting] = useState<DocumentFormat | null>(null);
  const [savedFormat, setSavedFormat] = useState<DocumentFormat | null>(null);
  const [noteState, setNoteState] = useState<"idle" | "sending" | "error">("idle");

  const handleExportClick = async (format: DocumentFormat) => {
    setExporting(format);
    try {
      const firstLine =
        text
          .trim()
          .split("\n")[0]
          .replace(/^[#\s*-]+/, "")
          .slice(0, 40) || t("chat.exportDefaultTitle");
      await onExport(text, firstLine, format);
      setSavedFormat(format);
    } finally {
      setExporting(null);
    }
  };

  // Spec-Reviewer-Fund (Spec 0055, Review des Gesamtpakets): ein per
  // Mausklick betätigter `<button>` bekommt in WKWebView (macOS/Tauri)
  // standardmäßig KEINEN Fokus — `focus-within` greift für einen
  // Maus-Klick auf "In Notiz übernehmen"/"Als Markdown" also nie. Bewegt
  // der Nutzer danach die Maus weg, würde die Leiste (inkl. Erfolgs-/
  // Fehler-Rückmeldung) ohne dieses Flag sofort unsichtbar — ein
  // fehlgeschlagenes "In Notiz übernehmen" bliebe damit faktisch
  // unbemerkt, der Nutzer glaubt, es sei geklappt. Erzwingt die Leiste
  // sichtbar, solange irgendeine Aktion läuft, fehlgeschlagen ist, oder
  // gerade eine Erfolgsbestätigung zeigt.
  const forceVisible = exporting !== null || savedFormat !== null || noteState !== "idle";

  return (
    <div className="group max-w-[85%] narrow:max-w-[95%] min-w-0 space-y-2 rounded-lg bg-surface-raised p-3 text-sm text-fg shadow-sm">
      <div className="prose prose-sm prose-invert max-w-none prose-pre:bg-surface-sunken prose-p:my-1 prose-ul:my-1 prose-ol:my-1 prose-headings:my-1.5">
        <ReactMarkdown remarkPlugins={[remarkGfm]} components={MARKDOWN_COMPONENTS}>{text}</ReactMarkdown>
      </div>
      {/* Spec 0080, A3, zweiter Punkt: keine Export-/Notiz-Leiste bei
       * leerem Text — anders als die (bewusst verworfene) Mindestlängen-
       * Sperre aus Spec 0055 (s. `AssistantMessageView`-Moduldoc oben, "NICHT
       * zusätzlich unterhalb einer Mindestlänge") geht es hier nicht um
       * "kurz, aber ggf. notizwürdig", sondern um LEER: eine abgeschnittene
       * Antwort ohne jeden Text (z. B. über `onChatResponseTruncated` neu
       * angelegt, s. `ChatPanel`s Event-Handler) hat nichts, das exportiert
       * oder in eine Notiz übernommen werden könnte. */}
      {text.trim().length > 0 && (
        <div
          className={`flex flex-wrap items-center gap-2 border-t border-border/60 pt-2 text-xs transition-opacity duration-150 group-hover:opacity-100 focus-within:opacity-100 ${
            forceVisible ? "opacity-100" : "opacity-0"
          }`}
        >
          <TakeIntoNoteButton sessionId={sessionId} content={text} onStateChange={setNoteState} />
          <span className="text-fg-subtle">Export:</span>
          <Button
            size="sm"
            disabled={exporting !== null}
            onClick={() => handleExportClick("markdown")}
          >
            {exporting === "markdown" ? t("common.saving") : `📄 ${t("chat.exportAsMarkdown")}`}
          </Button>
          {savedFormat && <span className="text-xs text-success-fg">✓ {t("chat.exportedAsMarkdown")}</span>}
        </div>
      )}
      {truncated ? (
        // Spec 0065, Teil 2: dauerhaft sichtbar (nicht Teil der
        // Hover-Leiste oben) — ein abgebrochener Antworttext ist keine
        // beiläufige Zusatzfunktion, sondern der Grund, warum die Antwort
        // gerade unvollständig endet. Reines UI-Element aus dem `truncated`-
        // Flag, nicht Teil von `text` (nicht fälschbar, s. Feld-Kommentar).
        <div className="flex flex-wrap items-center gap-2 border-t border-warning-border-strong/40 pt-2 text-xs text-warning-fg-soft">
          <span>✂ {t("chat.responseTruncated")}</span>
          <Button size="sm" onClick={onContinue}>
            {t("chat.continue")}
          </Button>
        </div>
      ) : null}
    </div>
  );
}

