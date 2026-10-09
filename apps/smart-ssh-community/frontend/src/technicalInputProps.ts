/**
 * Issue #217: props for technical text fields (host, user, paths, keys,
 * model names, filter patterns, ...). They switch off the system text
 * correction (auto-capitalization, autocorrect, spell check), which the
 * macOS webview applies to plain inputs by default. Free-text fields (chat
 * input, notes) must NOT spread this.
 *
 * Usage: `<input {...TECHNICAL_INPUT_PROPS} ... />`
 */
export const TECHNICAL_INPUT_PROPS = {
  autoCapitalize: "off",
  autoCorrect: "off",
  spellCheck: false,
} as const;
