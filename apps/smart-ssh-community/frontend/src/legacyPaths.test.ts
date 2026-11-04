// Issue #916: the old flat paths stay importable as deprecated re-export stubs
// so embedding apps keep building. Importing each one also type-checks it
// (`tsc -b` covers test files). Stubs: api, types, languages, navigationBus,
// fileDialog, technicalInputProps, resolveSectionLabel (i18n and the other
// listed modules did not move).
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import * as oldApi from "./api";
import * as newApi from "./api/api";
import * as oldTypes from "./types";
import * as newTypes from "./api/types";
import * as oldLanguages from "./languages";
import * as newLanguages from "./lib/languages";
import * as oldNav from "./navigationBus";
import * as newNav from "./buses/navigationBus";
import * as oldFileDialog from "./fileDialog";
import * as newFileDialog from "./api/fileDialog";
import * as oldInput from "./technicalInputProps";
import * as newInput from "./lib/technicalInputProps";
import * as oldSection from "./resolveSectionLabel";
import * as newSection from "./lib/resolveSectionLabel";

const PAIRS: [string, object, object][] = [
  ["api", oldApi, newApi],
  ["types", oldTypes, newTypes],
  ["languages", oldLanguages, newLanguages],
  ["navigationBus", oldNav, newNav],
  ["fileDialog", oldFileDialog, newFileDialog],
  ["technicalInputProps", oldInput, newInput],
  ["resolveSectionLabel", oldSection, newSection],
];

describe("deprecated re-export stubs", () => {
  it.each(PAIRS)("%s re-exports the moved module", (_name, oldMod, newMod) => {
    expect(Object.keys(oldMod).sort()).toEqual(Object.keys(newMod).sort());
    for (const key of Object.keys(newMod)) {
      expect((oldMod as Record<string, unknown>)[key]).toBe((newMod as Record<string, unknown>)[key]);
    }
  });
});
