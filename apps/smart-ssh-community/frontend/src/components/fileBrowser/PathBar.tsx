import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { displayPath } from "../../lib/remotePath";
import { TECHNICAL_INPUT_PROPS } from "../../lib/technicalInputProps";
import { Button, FIELD_CONTROL_CLASS, Icon } from "../ui";

/** One breadcrumb segment: its label and the path it loads. */
export interface PathSegment {
  label: string;
  path: string;
}

/** Splits a remote path into clickable ancestors. `"/"` is one segment, `"."`
 * (start directory) shows as `~`; relative paths keep their `.` prefix. */
export function pathSegments(path: string): PathSegment[] {
  if (path === "." || path === "") return [{ label: displayPath("."), path: "." }];
  const absolute = path.startsWith("/");
  const parts = path.split("/").filter((p) => p !== "");
  const out: PathSegment[] = absolute ? [{ label: "/", path: "/" }] : [];
  let acc = absolute ? "" : null;
  for (const part of parts) {
    acc = acc === null ? part : `${acc}/${part}`;
    out.push({ label: part === "." ? displayPath(".") : part, path: acc });
  }
  return out;
}

interface PathBarProps {
  narrow: boolean;
  path: string;
  pathInput: string;
  setPathInput: (value: string) => void;
  /** Resolves to whether the directory loaded. */
  load: (path: string) => void | Promise<boolean>;
  editing: boolean;
  setEditing: (editing: boolean) => void;
}

/** Breadcrumb path bar that turns into the editable path input (Spec 0020 §5). */
export function PathBar({ narrow, path, pathInput, setPathInput, load, editing, setEditing }: PathBarProps) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  const height = narrow ? "min-h-11" : "h-8";

  useEffect(() => {
    if (editing) {
      // Ctrl/Cmd+L enters edit mode without going through `startEditing`.
      setPathInput(path);
      inputRef.current?.focus();
      inputRef.current?.select();
    }
    // Only when edit mode starts; typing must not reset the input.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editing]);

  const startEditing = () => {
    setPathInput(path);
    setEditing(true);
  };

  if (editing) {
    return (
      <form
        className="min-w-32 flex-1"
        onSubmit={(e) => {
          e.preventDefault();
          // Stay in edit mode (typed text kept) until the load succeeded.
          void Promise.resolve(load(pathInput.trim() || ".")).then((ok) => {
            if (ok !== false) setEditing(false);
          });
        }}
      >
        <input
          {...TECHNICAL_INPUT_PROPS}
          ref={inputRef}
          value={pathInput}
          onChange={(e) => setPathInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              setPathInput(path);
              setEditing(false);
            }
          }}
          onBlur={() => {
            setPathInput(path);
            setEditing(false);
          }}
          aria-label={t("fileBrowser.properties.path")}
          title={pathInput}
          className={`${FIELD_CONTROL_CLASS} ${height} w-full px-2 py-1 font-mono text-xs`}
        />
      </form>
    );
  }

  const segments = pathSegments(path);
  return (
    <nav
      aria-label={t("fileBrowser.properties.path")}
      title={path}
      data-testid="path-bar"
      className={`flex min-w-32 flex-1 items-center overflow-hidden rounded border border-border bg-surface-sunken px-1 font-mono text-xs ${height}`}
    >
      <ol className="m-0 flex min-w-0 list-none items-center p-0">
        {segments.map((segment, i) => {
          const last = i === segments.length - 1;
          return (
            <li key={segment.path} className="flex min-w-0 items-center">
              {i > 0 && segments[i - 1].label !== "/" && (
                <Icon name="chev-r" className="shrink-0 text-fg-faint" />
              )}
              <Button
                variant="bare"
                // The last segment is the current directory: clicking it edits the path.
                onClick={() => (last ? startEditing() : load(segment.path))}
                aria-current={last ? "location" : undefined}
                title={segment.path}
                className={`max-w-48 truncate rounded px-1.5 py-0.5 focus-ring-flush ${
                  narrow ? "min-h-9" : ""
                } ${last ? "text-fg" : "text-fg-subtle hover:bg-surface-hover hover:text-fg-soft"}`}
              >
                {segment.label}
              </Button>
            </li>
          );
        })}
      </ol>
      {/* The empty part of the bar switches to the input; keyboard users use Ctrl/Cmd+L or the last segment. */}
      <Button
        variant="bare"
        tabIndex={-1}
        aria-hidden="true"
        data-testid="path-bar-edit"
        onClick={startEditing}
        className="min-w-4 flex-1 cursor-text self-stretch"
      />
    </nav>
  );
}
