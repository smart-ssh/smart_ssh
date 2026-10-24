/** Issue #653: text of the file list's Owner column: `user:group`, with the
 * numeric id for a side whose name is unknown; "—" when nothing is known. */
export function formatOwner(entry: {
  owner: string | null;
  group: string | null;
  uid: number | null;
  gid: number | null;
}): string {
  const user = entry.owner ?? (entry.uid !== null ? String(entry.uid) : null);
  const group = entry.group ?? (entry.gid !== null ? String(entry.gid) : null);
  if (user === null && group === null) return "—";
  return `${user ?? "—"}:${group ?? "—"}`;
}
