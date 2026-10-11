import type { ChatActionOutputEvent } from "./types";

/** Issue #325 (Spec 0106): live output of a running command, display only.
 * Replaced by the final result as soon as `chat-action-result` arrives. */
export interface LiveOutput {
  stdout: string;
  stderr: string;
  truncated: boolean;
}

/** Only the most recent part of each stream is kept on screen, so a command
 * that prints megabytes does not slow the chat down. The final result
 * shows the (capped) full output. */
export const LIVE_OUTPUT_TAIL_CHARS = 20_000;

function keepTail(text: string): string {
  if (text.length <= LIVE_OUTPUT_TAIL_CHARS) return text;
  const cut = text.slice(text.length - LIVE_OUTPUT_TAIL_CHARS);
  // Start at a line start where possible, so no half line is shown.
  const lineStart = cut.search(/[\r\n]/);
  return lineStart >= 0 && lineStart < cut.length - 1 ? cut.slice(lineStart + 1) : cut;
}

/** Appends the newly released text of one `chat-action-output` event. */
export function appendLiveOutput(
  previous: LiveOutput | undefined,
  event: Pick<ChatActionOutputEvent, "stdout" | "stderr" | "truncated">,
): LiveOutput {
  return {
    stdout: keepTail((previous?.stdout ?? "") + event.stdout),
    stderr: keepTail((previous?.stderr ?? "") + event.stderr),
    truncated: (previous?.truncated ?? false) || event.truncated,
  };
}
