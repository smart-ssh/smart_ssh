import { useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { QuestionOption } from "../types";

/** Issue #547 (Spec 0111 §5): lock state of a question card, derived from
 * the chat item list by `deriveQuestionStates`. */
export interface QuestionState {
  /** `true` once a user message followed, or a newer question exists. */
  locked: boolean;
  /** Labels of the options the answer matched (empty when not answered by
   * option). */
  selected: string[];
  /** The question was answered, but by a message matching no option. */
  ownWords: boolean;
}

/** The AI's question with its answer options. All text is untrusted and
 * rendered as plain text. An option click only calls `onAnswer` with the
 * label text(s) — the caller sends it as a normal user message; nothing here
 * can execute or approve a command. */
export function QuestionCard({
  question,
  options,
  multiSelect,
  state,
  onAnswer,
}: {
  question: string;
  options: QuestionOption[];
  multiSelect: boolean;
  state: QuestionState;
  onAnswer: (text: string) => void;
}) {
  const { t } = useTranslation();
  const [ticked, setTicked] = useState<string[]>([]);
  const { locked } = state;
  const marked = locked ? state.selected : ticked;

  const toggle = (label: string) =>
    setTicked((prev) =>
      prev.includes(label) ? prev.filter((l) => l !== label) : [...prev, label],
    );
  const submitMulti = () => {
    const text = options
      .map((o) => o.label)
      .filter((l) => ticked.includes(l))
      .join("\n");
    if (text) onAnswer(text);
  };
  const choose = (label: string) => {
    if (locked) return;
    if (multiSelect) toggle(label);
    else onAnswer(label);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    if (locked || e.ctrlKey || e.metaKey || e.altKey) return;
    if (!/^[1-4]$/.test(e.key)) return;
    const option = options[Number(e.key) - 1];
    if (!option) return;
    e.preventDefault();
    choose(option.label);
  };

  return (
    <div role="presentation" onKeyDown={onKeyDown} className="max-w-[90%]">
      <fieldset
        aria-label={t("chat.question.cardLabel", { question })}
        className={`m-0 min-w-0 rounded border px-3 py-2 text-sm ${
          locked
            ? "border-border bg-surface/60"
            : "border-accent-strong/60 bg-surface"
        }`}
      >
        <p className="whitespace-pre-wrap break-words font-medium text-fg">
          {question}
        </p>
        <ul className="mt-2 space-y-1">
          {options.map((o, i) => {
            const isMarked = marked.includes(o.label);
            return (
              <li key={`${i}-${o.label}`}>
                <button
                  type="button"
                  disabled={locked}
                  aria-pressed={multiSelect ? isMarked : undefined}
                  onClick={() => choose(o.label)}
                  className={`flex w-full items-start gap-2 rounded border px-2 py-1 text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg ${
                    isMarked
                      ? "border-accent-fg bg-accent-subtle-hover/60 text-accent-fg-faintest"
                      : "border-border text-fg-soft enabled:hover:bg-surface-raised"
                  } disabled:cursor-default`}
                >
                  <span aria-hidden="true" className="w-4 shrink-0 text-center">
                    {locked
                      ? isMarked
                        ? "✓"
                        : ""
                      : multiSelect
                        ? isMarked
                          ? "☑"
                          : "☐"
                        : i + 1}
                  </span>
                  <span className="min-w-0">
                    <span className="block break-words">{o.label}</span>
                    {o.description && (
                      <span className="block break-words text-xs text-fg-subtle">
                        {o.description}
                      </span>
                    )}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
        {multiSelect && !locked && (
          <button
            type="button"
            disabled={ticked.length === 0}
            onClick={submitMulti}
            className="mt-2 rounded bg-accent px-3 py-1 text-xs text-fg-bright focus:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg-soft enabled:hover:bg-accent-strong disabled:opacity-50"
          >
            {t("chat.question.send")}
          </button>
        )}
        {locked && state.ownWords && (
          <p className="mt-2 text-xs text-fg-subtle">
            {t("chat.question.answeredOwnWords")}
          </p>
        )}
      </fieldset>
    </div>
  );
}

/** Issue #547: the single source of truth for which question cards are
 * active and how locked ones are marked — used for live chats and for
 * reopened history alike. A question is answered by the first user message
 * after it; only the latest question without one stays active. */
export function deriveQuestionStates(
  items: readonly { type: string; id: string }[],
): Map<string, QuestionState> {
  const states = new Map<string, QuestionState>();
  const qs = items as readonly {
    type: string;
    id: string;
    text?: string;
    options?: QuestionOption[];
    multiSelect?: boolean;
  }[];
  qs.forEach((item, idx) => {
    if (item.type !== "question") return;
    let answer: string | null = null;
    let superseded = false;
    for (let j = idx + 1; j < qs.length; j++) {
      if (qs[j].type === "user") {
        answer = qs[j].text ?? "";
        break;
      }
      if (qs[j].type === "question") {
        superseded = true;
        break;
      }
    }
    if (answer === null) {
      states.set(item.id, {
        locked: superseded,
        selected: [],
        ownWords: false,
      });
      return;
    }
    const labels = (item.options ?? []).map((o) => o.label);
    let selected: string[] = [];
    if (item.multiSelect) {
      const lines = answer.split("\n");
      if (lines.length > 0 && lines.every((l) => labels.includes(l))) {
        selected = labels.filter((l) => lines.includes(l));
      }
    } else if (labels.includes(answer)) {
      selected = [answer];
    }
    states.set(item.id, {
      locked: true,
      selected,
      ownWords: selected.length === 0,
    });
  });
  return states;
}
