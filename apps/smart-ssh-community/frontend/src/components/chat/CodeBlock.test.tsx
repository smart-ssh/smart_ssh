import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import {
  act,
  render,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { testI18n } from "../../testI18n";
import ReactMarkdown from "react-markdown";
import { MARKDOWN_COMPONENTS } from "./DocumentCard";
import { clipboardWriteText } from "../../api/api";

vi.mock("../../api/api", async () =>
  (await import("../../test/apiMock")).createApiMock({
    clipboardWriteText: vi.fn(),
  }),
);

const CODE = 'line1 $HOME \\n "q" <a>\n\tindented';
const md = (s: string) =>
  render(
    <I18nextProvider i18n={testI18n}>
      <ReactMarkdown components={MARKDOWN_COMPONENTS}>{s}</ReactMarkdown>
    </I18nextProvider>,
  );

describe("code block copy button", () => {
  beforeEach(async () => {
    await testI18n.changeLanguage("en");
    vi.mocked(clipboardWriteText).mockReset().mockResolvedValue(undefined);
  });
  afterEach(() => vi.useRealTimers());

  it("is present on a fenced block and absent on inline code", () => {
    md("Use `ls -la` here\n\n```sh\necho hi\n```\n");
    expect(screen.getAllByRole("button", { name: "Copy code" })).toHaveLength(
      1,
    );
  });

  it("copies exactly the block text, each block its own", async () => {
    md("```sh\n" + CODE + "\n```\n\ntext\n\n```\nsecond\n```\n");
    const buttons = screen.getAllByRole("button", { name: "Copy code" });
    fireEvent.click(buttons[0]);
    expect(clipboardWriteText).toHaveBeenLastCalledWith(CODE);
    fireEvent.click(buttons[1]);
    expect(clipboardWriteText).toHaveBeenLastCalledWith("second");
    await screen.findAllByRole("button", { name: "Code copied" });
  });

  it("shows the check mark, then returns to the copy icon", async () => {
    vi.useFakeTimers();
    md("```\nx\n```\n");
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Copy code" })),
    );
    expect(screen.getByRole("button", { name: "Code copied" })).toBeTruthy();
    act(() => void vi.advanceTimersByTime(2100));
    expect(screen.getByRole("button", { name: "Copy code" })).toBeTruthy();
  });

  it("shows an error state when the write fails", async () => {
    vi.mocked(clipboardWriteText).mockRejectedValue(new Error("nope"));
    md("```\nx\n```\n");
    fireEvent.click(screen.getByRole("button", { name: "Copy code" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Copy failed" })).toBeTruthy(),
    );
  });
});
