import { type Dispatch, type FormEvent, type RefObject, type SetStateAction, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ServerFormTabSections, ServerFormSections } from "../../extensions/FormSlots";
import {
  convertIdentityFileToKeychain,
  createServer,
  previewEffectiveNotes,
  updateServer,
} from "../../api";
import { commandErrorText } from "../../errorCodes";
import type { GroupDto, ServerDto } from "../../types";
import { HostKeyDialog } from "../HostKeyDialog";
import { NotesPanel } from "../NotesPanel";
import { TestConnectionDialog } from "../TestConnectionDialog";
import { Button, Tabs, tabId } from "../ui";
import { ConnectionTab } from "./ConnectionTab";
import { DeleteServerSection } from "./DeleteServerSection";
import { ExtensionTabPanels } from "./ExtensionTabPanels";
import type { ConnectionTestState, IdentityFileState, ServerDeleteState, ServerModel } from "./formTypes";
import { GeneralTab } from "./GeneralTab";
import { ImportKeyDialog } from "./ImportKeyDialog";
import { NotesTab } from "./NotesTab";
import { SecurityTab } from "./SecurityTab";
import type { ServerFormTabs } from "./useServerFormTabs";

interface Props {
  serverId: string | null;
  allGroups: GroupDto[];
  possibleJumpHosts: ServerDto[];
  model: ServerModel;
  tabs: ServerFormTabs;
  test: ConnectionTestState;
  del: ServerDeleteState;
  identity: IdentityFileState;
  formRef: RefObject<HTMLFormElement | null>;
  error: string | null;
  setError: (message: string | null) => void;
  preview: { text: string | null; loading: boolean };
  setPreview: Dispatch<SetStateAction<{ text: string | null; loading: boolean }>>;
  onSaved: () => void;
  onDeleted: () => void;
  onConnect?: (serverId: string) => void;
  autoFocusNotes: boolean;
}

/** A remote server (new or saved): all tabs plus the extension tabs. */
export function RemoteServerForm({
  serverId,
  allGroups,
  possibleJumpHosts,
  model,
  tabs,
  test,
  del,
  identity,
  formRef,
  error,
  setError,
  preview,
  setPreview,
  onSaved,
  onDeleted,
  onConnect,
  autoFocusNotes,
}: Props) {
  const { t } = useTranslation();
  const isCreate = serverId === null;
  const { loaded, dirty, auth, startDirectoryCheck, buildInput, markDirty, markSaved, clearSudoPassword } = model;
  const { tabsIdBase, activeTab, availableTabs, tabItems, invalidTabs, setActiveTab, setInvalidTabs, collectInvalidTabs } =
    tabs;
  const { testing, testResult, testDialog, pendingHostKey, setPendingHostKey, cancelTest, closeTestDialog, handleTrustAndRetest } =
    test;
  const { importSavedId, setImportError, setImportSavedId, setImportPromptOpen, finishKeepingPath } = identity;
  const [saving, setSaving] = useState(false);

  const startDirectoryError = startDirectoryCheck.ok
    ? null
    : t(`serverForm.startDirectoryInvalid.${startDirectoryCheck.reason}`);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const invalid = collectInvalidTabs();
    if (startDirectoryError) invalid.add("connection");
    setInvalidTabs(invalid);
    if (invalid.size > 0) {
      const first = availableTabs.find((id) => invalid.has(id));
      if (first) setActiveTab(first);
      if (startDirectoryError) setError(startDirectoryError);
      tabs.requestReportValidity();
      return;
    }
    // Issue #667: a key-file server that is new or whose path changed asks
    // once whether to import the key or keep using the file path.
    if (
      auth.kind === "identityFile" &&
      auth.path.trim() !== "" &&
      !(loaded?.authKind === "identity_file" && loaded.identityFilePath === auth.path)
    ) {
      setImportError(null);
      setImportSavedId(null);
      setImportPromptOpen(true);
      return;
    }
    await save(false);
  };

  /** Saves the form; with `importKey` the key file is then imported through
   * the existing conversion (`convertIdentityFileToKeychain`). */
  const save = async (importKey: boolean) => {
    setSaving(true);
    setError(null);
    setImportError(null);
    let savedId = importSavedId;
    try {
      if (savedId === null) {
        if (isCreate) {
          savedId = await createServer(buildInput());
        } else if (serverId) {
          await updateServer(serverId, buildInput());
          savedId = serverId;
        }
      }
      if (importKey && savedId) {
        setImportSavedId(savedId);
        await convertIdentityFileToKeychain(savedId);
      }
      setImportPromptOpen(false);
      setImportSavedId(null);
      markSaved();
      setInvalidTabs(new Set());
      onSaved();
    } catch (err) {
      const message = commandErrorText(t, err);
      // The server itself is saved once `importSavedId` is set; only the
      // import failed — keep the dialog open for a retry or "keep path".
      if (importKey && savedId !== null) {
        setImportError(message);
      } else {
        setError(message);
        setImportPromptOpen(false);
      }
    } finally {
      setSaving(false);
    }
  };

  const importPrimaryRef = useRef<HTMLButtonElement>(null);

  // Cancel/Escape of the key-import prompt; after a failed import the server is already saved, so it acts like "keep".
  const cancelImportPrompt = () => {
    if (saving) return;
    if (importSavedId !== null) finishKeepingPath();
    else setImportPromptOpen(false);
  };

  const handleClearSudoPassword = async () => {
    if (!serverId) return;
    setError(null);
    await clearSudoPassword((err) => {
      // Spec 0071, A17 (spec-reviewer-Fund): Der generische
      // KEYCHAIN_UNAVAILABLE-Text spricht von „speichern oder lesen" — das
      // Entscheidende auf diesem Pfad sagt er nicht. Der Nutzer muss
      // wissen, dass das Passwort weiter im Schlüsselbund liegt und beim
      // nächsten `sudo` erneut eingespeist wird; genau das ist die
      // Begründung, aus der A17 diesen Fall überhaupt scheitern lässt.
      setError(t("serverForm.removeSudoPasswordFailed") + " " + commandErrorText(t, err));
    });
  };

  const handlePreview = async () => {
    if (!serverId) return;
    setPreview((prev) => ({ ...prev, loading: true }));
    setError(null);
    try {
      setPreview({ text: await previewEffectiveNotes(serverId), loading: false });
    } catch (err) {
      setError(commandErrorText(t, err));
    } finally {
      setPreview((prev) => ({ ...prev, loading: false }));
    }
  };

  // Spec 0032, Abschnitt 3/5: host/port/username/auth/jump-host sind für

  return (
    <div className="max-w-2xl space-y-6 p-4">
      <h2 className="font-heading text-lg font-semibold tracking-wide text-fg">
        {isCreate ? t("serverForm.titleNew") : t("serverForm.titleExisting", { name: loaded?.name ?? "" })}
      </h2>

      <Tabs
        tabs={tabItems}
        value={activeTab}
        onChange={setActiveTab}
        ariaLabel={t("serverForm.tabs.label")}
        idBase={tabsIdBase}
      />

      <form
        ref={formRef}
        noValidate
        onSubmit={handleSubmit}
        onChange={() => {
          markDirty();
          if (invalidTabs.size > 0) setInvalidTabs(collectInvalidTabs());
        }}
        className="space-y-3"
      >
        <GeneralTab idBase={tabsIdBase} active={activeTab === "general"} model={model} allGroups={allGroups} />

        <ConnectionTab
          idBase={tabsIdBase}
          active={activeTab === "connection"}
          isCreate={isCreate}
          model={model}
          identity={identity}
          test={test}
          possibleJumpHosts={possibleJumpHosts}
          startDirectoryError={startDirectoryError}
          onClearSudoPassword={handleClearSudoPassword}
          importDialog={
            identity.importPromptOpen ? (
              <ImportKeyDialog
                model={model}
                identity={identity}
                saving={saving}
                importPrimaryRef={importPrimaryRef}
                cancelImportPrompt={cancelImportPrompt}
                save={save}
              />
            ) : null
          }
        />

        <SecurityTab idBase={tabsIdBase} active={activeTab === "security"} isCreate={isCreate} model={model} />

        {error && <p className="text-sm text-danger-fg">{error}</p>}

        {activeTab !== "notes" && (
          <div className="narrow:sticky narrow:bottom-0 narrow:z-10 narrow:-mx-4 narrow:border-t narrow:border-surface-raised narrow:bg-surface narrow:px-4 narrow:py-2">
            <div className="flex flex-wrap items-center gap-2 narrow:flex-col narrow:items-stretch">
              <Button variant="primary" type="submit" disabled={saving} className="narrow:min-h-11 narrow:w-full">
                {saving ? t("common.saving") : isCreate ? t("common.create") : t("common.save")}
              </Button>
              {!isCreate && serverId && loaded && onConnect && (
                <Button
                  type="button"
                  disabled={dirty || saving}
                  title={dirty ? t("serverForm.connectSaveFirst") : undefined}
                  onClick={() => onConnect(serverId)}
                  className="narrow:min-h-11 narrow:w-full"
                >
                  {t("serverForm.connect")}
                </Button>
              )}
              {!isCreate && dirty && onConnect && (
                <span className="text-xs text-fg-subtle" data-testid="connect-save-first-hint">
                  {t("serverForm.connectSaveFirst")}
                </span>
              )}
            </div>
          </div>
        )}
      </form>

      {!isCreate && serverId && loaded && (
        <>
          <ServerFormTabSections
            serverId={serverId}
            dirty={dirty}
            tab="connection"
            hidden={activeTab !== "connection"}
            labelledBy={tabId(tabsIdBase, "connection")}
          />
          <ServerFormTabSections
            serverId={serverId}
            dirty={dirty}
            tab="security"
            hidden={activeTab !== "security"}
            labelledBy={tabId(tabsIdBase, "security")}
          />
          <div hidden={activeTab !== "general"} className="space-y-6">
            <ServerFormSections serverId={serverId} dirty={dirty} tab="general" />
            <DeleteServerSection del={del} onDeleted={onDeleted} />
          </div>
          <NotesTab idBase={tabsIdBase} active={activeTab === "notes"} serverId={serverId} dirty={dirty}>
            <NotesPanel
              target={{ Server: serverId }}
              currentNotes={loaded.notes}
              onNotesChanged={onSaved}
              autoFocus={autoFocusNotes}
            />

            <div>
              <Button size="sm"
                onClick={handlePreview}
                disabled={preview.loading}
              >
                {preview.loading ? t("common.loading") : t("serverForm.contextPreview")}
              </Button>
              {preview.text !== null && (
                <pre className="mt-2 max-h-64 overflow-y-auto whitespace-pre-wrap rounded border border-border bg-surface-sunken p-3 text-xs text-fg-muted">
                  {preview.text || t("serverForm.noContext")}
                </pre>
              )}
            </div>
          </NotesTab>
          <ExtensionTabPanels
            idBase={tabsIdBase}
            serverId={serverId}
            tabs={tabs.shownExtTabs}
            mounted={tabs.mountedExtTabs}
            activeTab={activeTab}
          />
        </>
      )}

      {testDialog && (
        <TestConnectionDialog
          steps={testDialog.steps}
          result={testing ? null : testResult}
          onCancel={cancelTest}
          onClose={closeTestDialog}
        />
      )}

      {pendingHostKey && !testDialog && (
        <HostKeyDialog
          event={pendingHostKey}
          onDecision={(decision) => {
            if (decision.decision === "trust") {
              handleTrustAndRetest();
            } else {
              setPendingHostKey(null);
            }
          }}
        />
      )}
    </div>
  );
}
