import type { IconName } from "../ui";

/** Icon of an activity kind; unknown kinds get the generic activity icon. */
export function kindIcon(kind: string): IconName {
  return kind === "speech-model"
    ? "mic"
    : kind === "download"
      ? "download"
      : kind === "upload"
        ? "upload"
        : "activity";
}
