/** Test helpers for extensions and embedding apps (`smart-ssh/test-support`).
 * Not used by production code. */
export { installMatchMediaMock, setWide } from "./test/matchMedia";
export { setNarrow } from "./test/matchMedia";
export {
  listComposerActions,
  listComposerDropHandlers,
  resetComposerRegistryForTests,
} from "./extensions/registry";
export { resetTiersForTests } from "./extensions/tiers";
