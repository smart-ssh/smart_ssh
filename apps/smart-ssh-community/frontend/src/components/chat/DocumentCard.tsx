import { useState, useSyncExternalStore, type ComponentPropsWithoutRef } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useTranslation } from "react-i18next";
import { useIsNarrow } from "../../hooks/useIsNarrow";
import { listDocumentActions, subscribeDocumentActions } from "../../extensions/registry";
import type { DocumentFormat } from "../../api/types";
import { Button, ErrorBoundary } from "../ui";

/** Spec 0113 §4a: in the narrow layout a code block scrolls sideways inside its own box, so it
 * must be reachable by keyboard; the wide layout keeps the plain `pre`. */
function ScrollablePre(props: ComponentPropsWithoutRef<"pre">) {
  const narrow = useIsNarrow();
  // A scrollable region must be keyboard-reachable (axe "scrollable-region-focusable").
  // oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex
  return <pre {...props} tabIndex={narrow ? 0 : undefined} />;
}
export const MARKDOWN_COMPONENTS = { pre: ({ node: _node, ...rest }: ComponentPropsWithoutRef<"pre"> & { node?: unknown }) => <ScrollablePre {...rest} /> };

/** Spec 0012, Abschnitt 3: eigene Karte für `chat-document-generated`,
 * gerendertes Markdown statt Rohtext, mit den beiden Export-Buttons. Kein
 * Bestätigungsdialog/keine Approve-Deny-Buttons wie bei `type: "action"` —
 * es ist bereits nichts Persistentes passiert, das rückgängig zu machen
 * wäre. */
export function DocumentCard({
  title,
  contentMarkdown,
  onExport,
}: {
  title: string;
  contentMarkdown: string;
  onExport: (contentMarkdown: string, title: string, format: DocumentFormat) => Promise<void>;
}) {
  const { t } = useTranslation();
  const documentActions = useSyncExternalStore(subscribeDocumentActions, listDocumentActions);
  const [exporting, setExporting] = useState<DocumentFormat | null>(null);
  const [savedFormat, setSavedFormat] = useState<DocumentFormat | null>(null);

  const handleExportClick = async (format: DocumentFormat) => {
    setExporting(format);
    try {
      await onExport(contentMarkdown, title, format);
      setSavedFormat(format);
    } finally {
      setExporting(null);
    }
  };

  return (
    <div className="rounded-lg border border-accent-border-strong/60 bg-surface-raised/80 p-3.5 text-sm shadow-md">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <span className="text-base">📄</span>
          <span className="min-w-0 break-words font-semibold text-fg">{title}</span>
        </div>
        <span className="rounded bg-accent-subtle/80 px-2 py-0.5 text-xs text-accent-fg-soft border border-accent-border/50">
          {t("chat.documentGenerated")}
        </span>
      </div>
      <div className="prose prose-sm prose-invert max-w-none rounded bg-surface-sunken p-3 prose-pre:bg-surface prose-p:my-1 prose-ul:my-1 prose-ol:my-1 prose-headings:my-1.5">
        <ReactMarkdown remarkPlugins={[remarkGfm]} components={MARKDOWN_COMPONENTS}>{contentMarkdown}</ReactMarkdown>
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-2 narrow:[&_button]:min-h-11">
        <Button
          variant="primary"
          size="sm"
          disabled={exporting !== null}
          onClick={() => handleExportClick("markdown")}
        >
          {exporting === "markdown" ? t("common.saving") : t("chat.saveAsMarkdown")}
        </Button>
        {savedFormat && <span className="text-xs text-success-fg">✓ {t("chat.exportedAsMarkdown")}</span>}
        {/* Spec 0045: registrierte Dokument-Aktionen neben dem Markdown-
         * Export (reaktiv), der Export-Button bleibt an seinem Platz. */}
        {documentActions.map((action) => (
          <ErrorBoundary key={action.id} id={`document-action:${action.id}`}><Button
            size="sm"
            disabled={action.disabled}
            title={action.disabled ? action.disabledReason : undefined}
            onClick={() => action.onInvoke({ contentMarkdown, title })}
          >
            {action.label}
          </Button></ErrorBoundary>
        ))}
      </div>
    </div>
  );
}
