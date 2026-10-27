// Shared UI primitives (docs/ui-guidelines.md). Exported as one barrel so
// editions and extensions can use them too.
export { Button } from "./Button";
export type { ButtonProps, ButtonSize, ButtonVariant } from "./Button";
export { DialogFrame } from "./DialogFrame";
export type { DialogFrameProps } from "./DialogFrame";
export { Field, FIELD_CONTROL_CLASS } from "./Field";
export type { FieldControlProps, FieldProps } from "./Field";
export { Spinner } from "./Spinner";
export { TabPanel, Tabs, tabId, useTabsIdBase } from "./Tabs";
export type { TabItem } from "./Tabs";
export { Tooltip } from "./Tooltip";
export { ErrorBoundary } from "./ErrorBoundary";
export type { ErrorBoundaryProps } from "./ErrorBoundary";
