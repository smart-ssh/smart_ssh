import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ServerFormSections } from "../../extensions/FormSlots";
import {
  commandErrorMessage,
  largeNoteDialogThresholdChars,
  requestNoteShrink,
  updateLocalServerNotes,
  updateLocalServerTags,
} from "../../api";
import { commandErrorText } from "../../errorCodes";
import type { ServerDto } from "../../types";
import { Button, TabPanel, Tabs } from "../ui";
import { ExtensionTabPanels } from "./ExtensionTabPanels";
import type { ServerModel } from "./formTypes";
import { NotesTab } from "./NotesTab";
import { TagsField } from "./TagsField";
import type { ServerFormTabs } from "./useServerFormTabs";

interface Props {
  loaded: ServerDto;
  model: ServerModel;
  tabs: ServerFormTabs;
  error: string | null;
  setError: (message: string | null) => void;
  onSaved: () => void;
  autoFocusNotes: boolean;
}

/** The local pseudo-server (Spec 0032, section 3/5): host, port, username,
 * auth and jump host are meaningless for it (no `servers` row, no connection)
 * and hidden rather than disabled; only name (fixed "Localhost"), notes and
 * tags are editable. No delete button and no connection test. It has no
 * regular form submit (`create_server`/`update_server` reject its id): tags
 * and notes are saved through their own dedicated commands. */
export function LocalServerForm({ loaded, model, tabs, error, setError, onSaved, autoFocusNotes }: Props) {
  const { t } = useTranslation();
  const { tags, tagDraft, setTagDraft, addTag, removeTag, localNotes, setLocalNotes, dirty, markDirty, markSaved } = model;
  const { tabsIdBase, activeTab, tabItems, setActiveTab } = tabs;
  const [savingLocal, setSavingLocal] = useState({ notes: false, tags: false });
  const localNotesRef = useRef<HTMLTextAreaElement>(null);
  const hasAutoFocusedLocalNotes = useRef(false);
  useEffect(() => {
    if (autoFocusNotes && !hasAutoFocusedLocalNotes.current) {
      hasAutoFocusedLocalNotes.current = true;
      localNotesRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
      // `preventScroll`: s. identischer Kommentar in `NotesPanel.tsx`.
      localNotesRef.current?.focus({ preventScroll: true });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- auto-focus once per loaded server, guarded by a ref
  }, [loaded.id]);
  // Spec 0058, Teil 1 (Etappe 5): derselbe Hinweis wie `NotesPanel`, hier
  // für den lokalen Pseudo-Server separat nachgebaut (der `NotesPanel`
  // nicht verwendet, s. Kommentar am lokalen Zweig unten).
  const [largeLocalNoteThreshold, setLargeLocalNoteThreshold] = useState<number | null>(null);
  const [localShrink, setLocalShrink] = useState<{ requesting: boolean; error: string | null }>({
    requesting: false,
    error: null,
  });
  useEffect(() => {
    largeNoteDialogThresholdChars()
      .then(setLargeLocalNoteThreshold)
      .catch((err) => console.error(commandErrorMessage(err)));
  }, []);
  // Spec 0079, A4: Unicode-Skalarwerte (`[...text].length`), nicht Byte.
  const isLocalNoteLarge =
    largeLocalNoteThreshold !== null && [...localNotes].length >= largeLocalNoteThreshold;
  const handleSummarizeLocalNoteNow = async () => {
    setLocalShrink({ requesting: true, error: null });
    try {
      await requestNoteShrink(loaded.id);
      setLocalShrink((prev) => ({ ...prev, requesting: false }));
    } catch (err) {
      setLocalShrink({ requesting: false, error: commandErrorMessage(err) });
    }
  };

  const handleSaveLocalTags = async () => {
    setSavingLocal((prev) => ({ ...prev, tags: true }));
    setError(null);
    try {
      await updateLocalServerTags(tags);
      markSaved();
      onSaved();
    } catch (err) {
      setError(commandErrorText(t, err));
    } finally {
      setSavingLocal((prev) => ({ ...prev, tags: false }));
    }
  };

  const handleSaveLocalNotes = async () => {
    setSavingLocal((prev) => ({ ...prev, notes: true }));
    setError(null);
    try {
      await updateLocalServerNotes(localNotes);
      markSaved();
      onSaved();
    } catch (err) {
      setError(commandErrorText(t, err));
    } finally {
      setSavingLocal((prev) => ({ ...prev, notes: false }));
    }
  };


  return (
    <div className="max-w-2xl space-y-6 p-4" onChange={() => markDirty()}>
      <h2 className="font-heading text-lg font-semibold tracking-wide text-fg">{loaded.name}</h2>
      <p className="text-sm text-fg-subtle">{t("serverForm.localHint")}</p>

      {error && <p className="text-sm text-danger-fg">{error}</p>}

      <Tabs
        tabs={tabItems}
        value={activeTab}
        onChange={setActiveTab}
        ariaLabel={t("serverForm.tabs.label")}
        idBase={tabsIdBase}
      />

      <TabPanel idBase={tabsIdBase} id="general" active={activeTab === "general"} className="space-y-6">
        <TagsField
          tags={tags}
          tagDraft={tagDraft}
          setTagDraft={setTagDraft}
          addTag={addTag}
          removeTag={(tag) => {
            markDirty();
            removeTag(tag);
          }}
        >
          <Button variant="primary" size="sm"
            onClick={handleSaveLocalTags}
            disabled={savingLocal.tags}
            className="mt-2"
          >
            {savingLocal.tags ? t("common.saving") : t("common.save")}
          </Button>
        </TagsField>

        <ServerFormSections serverId={loaded.id} dirty={dirty} tab="general" />
      </TabPanel>

      <NotesTab idBase={tabsIdBase} active={activeTab === "notes"} serverId={loaded.id} dirty={dirty}>
        <div className="block text-sm text-fg-muted">
          {t("common.notes")}
          {isLocalNoteLarge && (
            <div className="mt-1 rounded border border-warning-border/50 bg-warning-subtle/30 px-3 py-2 text-xs text-warning-fg-soft">
              <p>{t("serverForm.largeNoteHint")}</p>
              <button
                type="button"
                onClick={handleSummarizeLocalNoteNow}
                disabled={localShrink.requesting}
                className="mt-1.5 underline hover:no-underline disabled:opacity-50"
              >
                {localShrink.requesting ? t("serverForm.summarizeRequesting") : t("serverForm.summarizeNow")}
              </button>
              {localShrink.error && <p className="mt-1 text-danger-fg">{localShrink.error}</p>}
            </div>
          )}
          <textarea
            ref={localNotesRef}
            value={localNotes}
            onChange={(e) => setLocalNotes(e.target.value)}
            rows={6}
            className="mt-1 w-full rounded border border-border-strong bg-surface px-2 py-1.5 text-sm text-fg"
          />
          <Button variant="primary" size="sm"
            onClick={handleSaveLocalNotes}
            disabled={savingLocal.notes}
            className="mt-2"
          >
            {savingLocal.notes ? t("common.saving") : t("common.save")}
          </Button>
        </div>
      </NotesTab>
      <ExtensionTabPanels
        idBase={tabsIdBase}
        serverId={loaded.id}
        tabs={tabs.shownExtTabs}
        mounted={tabs.mountedExtTabs}
        activeTab={activeTab}
      />
    </div>
  );
}
