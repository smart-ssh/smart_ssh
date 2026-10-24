import { commandErrorCode, commandErrorMessage } from "./api";

/** Issue #655: why the remote side of a text diff could not be read. */
export type PreviewReadFailure =
  | { kind: "tooLarge" }
  | { kind: "notText" }
  | { kind: "permission" }
  | { kind: "other"; message: string };

/** Classifies a failed `sftpReadText` by its stable error code. */
export function classifyPreviewReadFailure(err: unknown): PreviewReadFailure {
  switch (commandErrorCode(err)) {
    case "SFTP_PREVIEW_TOO_LARGE":
      return { kind: "tooLarge" };
    case "SFTP_PREVIEW_NOT_TEXT":
      return { kind: "notText" };
    case "SSH_SFTP_PERMISSION_DENIED":
      return { kind: "permission" };
    default:
      return { kind: "other", message: commandErrorMessage(err) };
  }
}

/** `t` key and options of the hint for a failure. */
export function previewReadFailureHint(failure: PreviewReadFailure): {
  key: string;
  options?: { message: string };
} {
  return failure.kind === "other"
    ? { key: "fileBrowser.readFailure.other", options: { message: failure.message } }
    : { key: `fileBrowser.readFailure.${failure.kind}` };
}
