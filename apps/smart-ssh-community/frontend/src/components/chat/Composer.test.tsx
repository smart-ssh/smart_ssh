import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../../testI18n";
import { Composer } from "./Composer";

function composer(sessionId: string) {
  return (
    <Composer
      draft=""
      attachments={[]}
      composerError={null}
      composerCtx={{ sessionId, addAttachment: vi.fn(), removeAttachment: vi.fn() }}
      composerActions={[]}
      invokeComposerAction={vi.fn()}
      inputRef={createRef()}
      chatAreaRef={createRef()}
      onSubmit={vi.fn()}
      onDraftChange={vi.fn()}
      onKeyDown={vi.fn()}
      voiceEnabled={false}
      voiceLocked={false}
      voiceHandlers={{ onStart: vi.fn(), onPartial: vi.fn(), onFinal: vi.fn(), onFailed: vi.fn() } as never}
      onVoicePhaseChange={vi.fn()}
    />
  );
}

describe("Composer label association", () => {
  it("gives every mounted composer its own labelled textarea", () => {
    render(
      <I18nextProvider i18n={testI18n}>
        {composer("a")}
        {composer("b")}
      </I18nextProvider>,
    );
    const areas = screen.getAllByRole("textbox") as HTMLTextAreaElement[];
    expect(areas).toHaveLength(2);
    expect(areas[0].id).not.toBe(areas[1].id);
    for (const area of areas) {
      expect(area.labels).toHaveLength(1);
    }
  });
});
