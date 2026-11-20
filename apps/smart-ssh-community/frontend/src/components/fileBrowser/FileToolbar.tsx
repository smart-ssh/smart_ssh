import type { ReactNode, RefObject } from "react";
import { useTranslation } from "react-i18next";
import { parentPath } from "../../lib/remotePath";
import { Button, Icon } from "../ui";
import { PathBar } from "./PathBar";

interface FileToolbarProps {
  narrow: boolean;
  path: string;
  startPath: string;
  pathInput: string;
  setPathInput: (value: string) => void;
  load: (path: string) => void;
  /** The path bar shows the editable input instead of the breadcrumb. */
  pathEditing: boolean;
  setPathEditing: (editing: boolean) => void;
  onRefresh: () => void;
  /** Opens the columns menu below the given point (the toolbar button's corner). */
  onShowColumns: (point: { x: number; y: number }) => void;
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
  pathEditing,
  setPathEditing,
  onRefresh,
  onShowColumns,
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
        <div className="flex items-center gap-1.5 border-b border-border px-2 py-1.5">
          <Button
            iconOnly
            onClick={() => load(parentPath(path))}
            disabled={path === "."}
            title={t("fileBrowser.parentDirectory")}
            aria-label={t("fileBrowser.parentDirectory")}
          >
            <Icon name="parent" />
          </Button>
          <PathBar
            narrow
            path={path}
            pathInput={pathInput}
            setPathInput={setPathInput}
            load={load}
            editing={pathEditing}
            setEditing={setPathEditing}
          />
          <div ref={overflowRef} className="shrink-0">
            <Button
              ref={overflowButtonRef}
              iconOnly
              onClick={() => {
                setElevationMenuOpen(false);
                setOtherUserOpen(false);
                setOverflowOpen((open) => !open);
              }}
              aria-haspopup="menu"
              aria-expanded={overflowOpen}
              aria-label={t("fileBrowser.directoryActions")}
              title={t("fileBrowser.directoryActions")}
            >
              <Icon name="more" />
            </Button>
            {overflowOpen && (
              <div
                role="menu"
                aria-label={t("fileBrowser.directoryActions")}
                className="fixed inset-x-0 bottom-0 z-30 max-h-[70vh] overflow-y-auto rounded-t border border-border-strong bg-surface-raised py-1 shadow-lg"
              >
                {[
                  {
                    key: "home",
                    label: t("fileBrowser.toStartDirectory"),
                    run: () => load(startPath),
                  },
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
                            : <><Icon name="lock" className="mr-1 inline align-text-bottom" />{t("fileElevation.toggle")}</>,
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
                  <Button variant="bare"
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
                  </Button>
                ))}
              </div>
            )}
            {otherUserForm}
          </div>
        </div>
      )}
      {!narrow && (
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5 border-b border-border px-2 py-1.5">
        <div className="flex min-w-[12rem] flex-1 items-center gap-1.5">
        <Button
          iconOnly
          onClick={() => load(parentPath(path))}
          disabled={path === "."}
          title={t("fileBrowser.parentDirectory")}
          aria-label={t("fileBrowser.parentDirectory")}
        >
          <Icon name="parent" />
        </Button>
        <Button
          iconOnly
          onClick={() => load(startPath)}
          title={t("fileBrowser.toStartDirectory")}
          aria-label={t("fileBrowser.toStartDirectory")}
        >
          <Icon name="home" />
        </Button>
        <Button
          iconOnly
          onClick={onRefresh}
          title={t("fileBrowser.menu.refresh")}
          aria-label={t("fileBrowser.menu.refresh")}
        >
          <Icon name="refresh" />
        </Button>
        <PathBar
          narrow={false}
          path={path}
          pathInput={pathInput}
          setPathInput={setPathInput}
          load={load}
          editing={pathEditing}
          setEditing={setPathEditing}
        />
        </div>
        <fieldset
          aria-label={t("fileBrowser.fileActionsGroup")}
          className="m-0 flex min-w-0 items-center gap-1.5 border-0 border-l border-border p-0 pl-2"
        >
        <Button onClick={onNewFolder}>
          {t("fileBrowser.newFolderButton")}
        </Button>
        <Button onClick={onUpload}>
          <Icon name="upload" />
          {t("fileBrowser.upload")}
        </Button>
        {folderPickerAvailable && (
          <Button onClick={onUploadFolder}>
            <Icon name="folder-open" />
            {t("fileBrowser.uploadFolder")}
          </Button>
        )}
        </fieldset>
        <Button
          iconOnly
          onClick={(e) => {
            const rect = e.currentTarget.getBoundingClientRect();
            onShowColumns({ x: rect.left, y: rect.bottom + 2 });
          }}
          aria-haspopup="menu"
          title={t("fileBrowser.columnsMenu")}
          aria-label={t("fileBrowser.columnsMenu")}
        >
          <Icon name="sliders" />
        </Button>
        {elevationControls}
        {cutEntry && (
          <>
            <Button
              onClick={onPaste}
              title={t("fileBrowser.pasteTitle", { name: cutEntry.name })}
            >
              {t("fileBrowser.paste")}
            </Button>
            <Button
              iconOnly
              onClick={onCancelCut}
              title={t("fileBrowser.cancelCut")}
              aria-label={t("fileBrowser.cancelCut")}
            >
              <Icon name="x" />
            </Button>
          </>
        )}
      </div>
      )}
    </>
  );
}
