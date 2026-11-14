import {
  Children,
  isValidElement,
  useEffect,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { clipboardWriteText } from "../../api/api";
import { Button } from "../ui";
import { useIsNarrow } from "../../hooks/useIsNarrow";

/** How long the check mark / error state stays before the copy icon returns. */
const FEEDBACK_MS = 2000;

/** Plain text of a rendered subtree (the code block's source text). */
function nodeText(node: ReactNode): string {
  if (node === null || node === undefined || typeof node === "boolean")
    return "";
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (isValidElement<{ children?: ReactNode }>(node))
    return nodeText(node.props.children);
  return Children.toArray(node).map(nodeText).join("");
}

/** The renderer ends a fenced block's text with one newline that is not part of the code. */
function codeBlockText(children: ReactNode): string {
  return nodeText(children).replace(/\n$/, "");
}

type CopyState = "idle" | "copied" | "failed";

/** Spec 0113 §4a: in the narrow layout a code block scrolls sideways inside its own box, so it
 * must be reachable by keyboard; the wide layout keeps the plain `pre`. The copy button sits in a
 * non-scrolling wrapper, so it stays put while the block scrolls (issue #1295). */
export function CodeBlock({
  children,
  ...rest
}: ComponentPropsWithoutRef<"pre">) {
  const { t } = useTranslation();
  const narrow = useIsNarrow();
  const [state, setState] = useState<CopyState>("idle");
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);

  const show = (next: CopyState) => {
    setState(next);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setState("idle"), FEEDBACK_MS);
  };
  // The backend write starts synchronously inside the click (no `await` before it), which keeps
  // it inside the user gesture on WebKit (issue #626).
  const handleCopy = () => {
    clipboardWriteText(codeBlockText(children)).then(
      () => show("copied"),
      () => show("failed"),
    );
  };
  const label =
    state === "failed"
      ? t("chat.codeCopyFailed")
      : state === "copied"
        ? t("chat.codeCopied")
        : t("chat.copyCode");

  return (
    <div className="relative">
      {/* A scrollable region must be keyboard-reachable (axe "scrollable-region-focusable"). */}
      <pre
        {...rest}
        className={`${rest.className ?? ""} pr-10!`}
        // oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex
        tabIndex={narrow ? 0 : undefined}
      >
        {children}
      </pre>
      <Button
        variant="bare"
        onClick={handleCopy}
        aria-label={label}
        title={label}
        className={`absolute top-1 right-1 flex h-7 w-7 items-center justify-center rounded text-fg-subtle hover:text-fg focus-ring-flush narrow:min-h-11 narrow:min-w-11 ${state === "failed" ? "text-danger-fg-soft" : ""}`}
      >
        <svg
          width="16"
          height="16"
          viewBox="0 0 16 16"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          {state === "copied" ? (
            <path d="M3 8.5l3 3 7-7" />
          ) : state === "failed" ? (
            <path d="M4 4l8 8M12 4l-8 8" />
          ) : (
            <>
              <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
              <path d="M10.5 5.5v-2a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2" />
            </>
          )}
        </svg>
      </Button>
    </div>
  );
}
