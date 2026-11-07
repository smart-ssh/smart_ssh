import { setUiPreference, settingsStore } from "../i18n";

/** Issue #1195: servers for which the user chose "Don't show again" in the
 * file browser's elevated-rights notice. A plain UI preference (it changes
 * no behaviour); the backend drops an id when the server is deleted. */
const ELEVATION_NOTICE_DISMISSED_KEY = "elevationNoticeDismissedServers";

/** Dismissed server ids; anything that is not a string array counts as none. */
export async function loadElevationNoticeDismissed(): Promise<string[]> {
  const store = await settingsStore();
  const stored = await store.get<unknown>(ELEVATION_NOTICE_DISMISSED_KEY);
  if (!Array.isArray(stored)) return [];
  return stored.filter((v): v is string => typeof v === "string");
}

/** Adds `serverId` to the list (once) and stores it. */
export async function dismissElevationNotice(serverId: string): Promise<void> {
  const current = await loadElevationNoticeDismissed();
  if (current.includes(serverId)) return;
  await setUiPreference(ELEVATION_NOTICE_DISMISSED_KEY, [...current, serverId]);
}
