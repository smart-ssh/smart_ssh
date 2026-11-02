import type { ReactNode, RefObject } from "react";
import { useTranslation } from "react-i18next";
import { parentPath } from "../../remotePath";
import { TECHNICAL_INPUT_PROPS } from "../../technicalInputProps";
import { Button, FIELD_CONTROL_CLASS } from "../ui";

interface FileToolbarProps {
  narrow: boolean;
  path: string;
  startPath: string;
  pathInput: string;
  setPathInput: (value: string) => void;
  load: (path: string) => void;
  /** Narrow layout: the "⋯" overflow menu. */
  overflowRef: RefObject<HTMLDivElement | null>;
  overflowButtonRef: RefObject<HTMLButtonElement | null>;
  overflowOpen: boolean;
  setOverflowOpen: (open: boolean | ((open: boolean) => boolean)) => void;
  setElevationMenuOpen: (open: boolean) => void;
  setOtherUserOpen: (open: boolean) => void;
  setOtherUserDraft: (user: string) => void;
  otherUserInputRef: RefObject<HTMLInputElement | null>;
  elevationTargetUser: string;
  /** The "other user" prompt (rendered by the parent). */
  otherUserForm: ReactNode;
  /** Wide layout: the `ElevationControls` element (null when elevation is not offered). */
  elevationControls: ReactNode;
  /** Elevation can be switched on (not elevated yet, session user is not root). */
  canElevate: boolean;
  enablingElevation: boolean;
  onToggleElevation: (targetUser: string) => void;
  folderPickerAvailable: boolean;
  onNewFolder: () => void;
  onUpload: () => void;
  onUploadFolder: () => void;
  /** The cut entry (null when nothing is cut). */
  cutEntry: { name: string } | null;
  onPaste: () => void;
  onCancelCut: () => void;
}

/** Path bar and action buttons of the file browser, narrow and wide layout. */
export function FileToolbar({
  narrow,
  path,
  startPath,
  pathInput,
  setPathInput,
  load,
  overflowRef,
  overflowButtonRef,
  overflowOpen,
  setOverflowOpen,
  setElevationMenuOpen,
  setOtherUserOpen,
  setOtherUserDraft,
  otherUserInputRef,
  elevationTargetUser,
  otherUserForm,
  elevationControls,
  canElevate,
  enablingElevation,
  onToggleElevation,
  folderPickerAvailable,
  onNewFolder,
  onUpload,
  onUploadFolder,
  cutEntry,
  onPaste,
  onCancelCut,
}: FileToolbarProps) {
  const { t } = useTranslation();
  return (
    <>
      {narrow && (
        <div className="flex items-center gap-1.5 border-b border-surface-raised px-2 py-1.5">
          <Button
            className="min-h-11 min-w-11"
            onClick={() => load(startPath)}
            title={t("fileBrowser.toStartDirectory")}
            aria-label={t("fileBrowser.toStartDirectory")}
          >
            ⌂
          </Button>
          <Button
            className="min-h-11 min-w-11"
            onClick={() => load(parentPath(path))}
            disabled={path === "."}
            title={t("fileBrowser.parentDirectory")}
            aria-label={t("fileBrowser.parentDirectory")}
          >
            ↑
          </Button>
          <form
            className="min-w-0 flex-1"
            onSubmit={(e) => {
              e.preventDefault();
              load(pathInput.trim() || ".");
            }}
          >
            <input
              {...TECHNICAL_INPUT_PROPS}
              value={pathInput}
              onChange={(e) => setPathInput(e.target.value)}
              aria-label={t("fileBrowser.properties.path")}
              title={pathInput}
              className={`${FIELD_CONTROL_CLASS} min-h-11 w-full px-2 py-1 font-mono text-xs`}
            />
          </form>
          <div ref={overflowRef} className="shrink-0">
            <Button
              ref={overflowButtonRef}
              className="min-h-11 min-w-11"
              onClick={() => {
                setElevationMenuOpen(false);
                setOtherUserOpen(false);
                setOverflowOpen((open) => !open);
              }}
              aria-haspopup="menu"
              aria-expanded={overflowOpen}
              aria-label={t("fileBrowser.fileActionsGroup")}
              title={t("fileBrowser.fileActionsGroup")}
            >
              ⋯
            </Button>
            {overflowOpen && (
              <div
                role="menu"
                aria-label={t("fileBrowser.fileActionsGroup")}
                className="fixed inset-x-0 bottom-0 z-30 max-h-[70vh] overflow-y-auto rounded-t border border-border-strong bg-surface-raised py-1 shadow-lg"
              >
                {[
                  {
                    key: "mkdir",
                    label: t("fileBrowser.newFolderButton"),
                    run: onNewFolder,
                  },
                  { key: "upload", label: t("fileBrowser.upload"), run: onUpload },
                  ...(folderPickerAvailable
                    ? [{ key: "uploadFolder", label: t("fileBrowser.uploadFolder"), run: onUploadFolder }]
                    : []),
                  ...(cutEntry
                    ? [
                        { key: "paste", label: t("fileBrowser.paste"), run: onPaste },
                        { key: "cancelCut", label: t("fileBrowser.cancelCut"), run: onCancelCut },
                      ]
                    : []),
                  ...(canElevate
                    ? [
                        {
                          key: "elevate",
                          label: enablingElevation
                            ? t("fileElevation.enabling")
                            : `🔓 ${t("fileElevation.toggle")}`,
                          run: () => onToggleElevation("root"),
                          disabled: enablingElevation,
                        },
                        {
                          key: "otherUser",
                          label: t("fileElevation.otherUser"),
                          run: () => {
                            setOtherUserDraft(elevationTargetUser);
                            setOtherUserOpen(true);
                            setTimeout(() => otherUserInputRef.current?.focus(), 0);
                          },
                          disabled: enablingElevation,
                        },
                      ]
                    : []),
                ].map((item) => (
                  <button
                    key={item.key}
                    type="button"
                    role="menuitem"
                    disabled={"disabled" in item ? item.disabled : false}
                    className="block min-h-11 w-full px-4 py-2 text-left text-sm text-fg-soft hover:bg-border focus-visible:bg-border focus-visible:outline-none disabled:opacity-50"
                    onClick={() => {
                      setOverflowOpen(false);
                      item.run();
                    }}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            )}
            {otherUserForm}
          </div>
        </div>
      )}
      {!narrow && (
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5 border-b border-surface-raised px-2 py-1.5">
        <div className="flex min-w-[12rem] flex-1 items-center gap-1.5">
        <Button
          size="sm"
          onClick={() => load(startPath)}
          title={t("fileBrowser.toStartDirectory")}
          aria-label={t("fileBrowser.toStartDirectory")}
        >
          ⌂
        </Button>
        <Button
          size="sm"
          onClick={() => load(parentPath(path))}
          disabled={path === "."}
          title={t("fileBrowser.parentDirectory")}
          aria-label={t("fileBrowser.parentDirectory")}
        >
          ↑
        </Button>
        <form
          className="min-w-32 flex-1"
          onSubmit={(e) => {
            e.preventDefault();
            load(pathInput.trim() || ".");
          }}
        >
          <input
            {...TECHNICAL_INPUT_PROPS}
            value={pathInput}
            onChange={(e) => setPathInput(e.target.value)}
            aria-label={t("fileBrowser.properties.path")}
            title={pathInput}
            className={`${FIELD_CONTROL_CLASS} w-full px-2 py-1 font-mono text-xs`}
          />
        </form>
        </div>
        <fieldset
          aria-label={t("fileBrowser.fileActionsGroup")}
          className="m-0 flex min-w-0 items-center gap-1.5 border-0 p-0"
        >
        <Button
          size="sm"
          onClick={onNewFolder}
        >
          {t("fileBrowser.newFolderButton")}
        </Button>
        <Button size="sm" onClick={onUpload}>
          {t("fileBrowser.upload")}
        </Button>
        {folderPickerAvailable && (
          <Button size="sm" onClick={onUploadFolder}>
            {t("fileBrowser.uploadFolder")}
          </Button>
        )}
        </fieldset>
        {elevationControls}
        {cutEntry && (
          <>
            <Button
              size="sm"
              onClick={onPaste}
              title={t("fileBrowser.pasteTitle", { name: cutEntry.name })}
            >
              {t("fileBrowser.paste")}
            </Button>
            <Button
              size="sm"
              onClick={onCancelCut}
              title={t("fileBrowser.cancelCut")}
              aria-label={t("fileBrowser.cancelCut")}
            >
              ✕
            </Button>
          </>
        )}
      </div>
      )}
    </>
  );
}
