import { describe, expect, it } from "vitest";
import { DEFAULT_FILE_SORT, nextFileSort, sortEntries, type FileSortColumn } from "./fileSort";
import type { RemoteEntryDto } from "./types";

function e(name: string, over: Partial<RemoteEntryDto> = {}): RemoteEntryDto {
  return {
    name,
    path: name,
    isDir: false,
    size: 0,
    permissions: "rw-r--r--",
    modified: null,
    permissionsOctal: 0o644,
    uid: 1,
    gid: 1,
    owner: null,
    group: null,
    ...over,
  };
}

const entries = [
  e("b.txt", { size: 300, permissions: "rw-r--r--", modified: "2024-03-01T00:00:00Z" }),
  e("a10.txt", { size: 100, permissions: "rwxr-xr-x", modified: "2024-01-01T00:00:00Z" }),
  e("A2.txt", { size: 200, permissions: "r--r--r--", modified: "2024-02-01T00:00:00Z" }),
  e("zdir", { isDir: true, size: 4096, permissions: "rwxr-xr-x", modified: "2024-01-05T00:00:00Z" }),
  e("Adir", { isDir: true, size: 10, permissions: "rwx------", modified: "2024-06-05T00:00:00Z" }),
];

const names = (sort: Parameters<typeof sortEntries>[1]) => sortEntries(entries, sort).map((x) => x.name);

describe("sortEntries", () => {
  const expected: Record<FileSortColumn, { asc: string[]; desc: string[] }> = {
    // natural + case-insensitive; folders on top, both groups sorted
    name: { asc: ["Adir", "zdir", "A2.txt", "a10.txt", "b.txt"], desc: ["zdir", "Adir", "b.txt", "a10.txt", "A2.txt"] },
    // folders are ordered by name (in the chosen direction)
    size: { asc: ["Adir", "zdir", "a10.txt", "A2.txt", "b.txt"], desc: ["zdir", "Adir", "b.txt", "A2.txt", "a10.txt"] },
    permissions: {
      asc: ["Adir", "zdir", "A2.txt", "b.txt", "a10.txt"],
      desc: ["zdir", "Adir", "a10.txt", "b.txt", "A2.txt"],
    },
    modified: { asc: ["zdir", "Adir", "a10.txt", "A2.txt", "b.txt"], desc: ["Adir", "zdir", "b.txt", "A2.txt", "a10.txt"] },
  };
  for (const column of Object.keys(expected) as FileSortColumn[]) {
    for (const direction of ["asc", "desc"] as const) {
      it(`${column} ${direction}, folders on top`, () => {
        const got = names({ column, direction });
        expect(got.slice(0, 2).every((n) => n.endsWith("dir"))).toBe(true);
        expect(got).toEqual(expected[column][direction]);
      });
    }
  }

  it("does not mutate its input", () => {
    const copy = [...entries];
    sortEntries(entries, { column: "size", direction: "desc" });
    expect(entries).toEqual(copy);
  });
});

describe("nextFileSort", () => {
  it("reverses on the same column and starts ascending on another", () => {
    const desc = nextFileSort(DEFAULT_FILE_SORT, "name");
    expect(desc).toEqual({ column: "name", direction: "desc" });
    expect(nextFileSort(desc, "name")).toEqual({ column: "name", direction: "asc" });
    expect(nextFileSort(desc, "size")).toEqual({ column: "size", direction: "asc" });
  });
});
