import type { CSSProperties, ReactNode, RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui";

interface ElevationControlsProps {
  groupRef: RefObject<HTMLFieldSetElement | null>;
  menuButtonRef: RefObject<HTMLButtonElement | null>;
  menuRef: RefObject<HTMLDivElement | null>;
  otherUserItemRef: RefObject<HTMLButtonElement | null>;
  otherUserInputRef: RefObject<HTMLInputElement | null>;
  menuStyle: CSSProperties | undefined;
  enablingElevation: boolean;
  elevationMenuOpen: boolean;
  otherUserOpen: boolean;
  elevationTargetUser: string;
  /** The "other user" prompt (rendered by the parent, shared with the narrow overflow menu). */
  otherUserForm: ReactNode;
  onToggleElevation: (targetUser: string) => void;
  setElevationMenuOpen: (open: boolean | ((open: boolean) => boolean)) => void;
  setOtherUserOpen: (open: boolean) => void;
  setOtherUserDraft: (user: string) => void;
}

/** Elevation button, its dropdown and the "other user" prompt slot (Spec 0067). */
export function ElevationControls({
  groupRef,
  menuButtonRef,
  menuRef,
  otherUserItemRef,
  otherUserInputRef,
  menuStyle,
  enablingElevation,
  elevationMenuOpen,
  otherUserOpen,
  elevationTargetUser,
  otherUserForm,
  onToggleElevation,
  setElevationMenuOpen,
  setOtherUserOpen,
  setOtherUserDraft,
}: ElevationControlsProps) {
  const { t } = useTranslation();
  return (
    <fieldset
      ref={groupRef}
      aria-label={t("fileElevation.group")}
      className="relative m-0 flex min-w-0 items-center gap-0.5 border-0 border-l border-border p-0 pl-2"
    >
      <Button
        size="sm"
        onClick={() => onToggleElevation("root")}
        disabled={enablingElevation}
        title={t("fileElevation.toggleTitle")}
      >
        {enablingElevation ? t("fileElevation.enabling") : `🔓 ${t("fileElevation.toggle")}`}
      </Button>
      <Button
        size="sm"
        ref={menuButtonRef}
        onClick={() => {
          setOtherUserOpen(false);
          setElevationMenuOpen((open) => !open);
        }}
        disabled={enablingElevation}
        aria-haspopup="menu"
        aria-expanded={elevationMenuOpen || otherUserOpen}
        aria-label={t("fileElevation.moreOptions")}
        title={t("fileElevation.moreOptions")}
      >
        ▾
      </Button>
      {elevationMenuOpen && (
        <div
          role="menu"
          ref={menuRef}
          style={menuStyle}
          className="fixed z-20 rounded border border-border-strong bg-surface-raised p-1 shadow-lg"
        >
          <button
            type="button"
            role="menuitem"
            ref={otherUserItemRef}
            className="w-full whitespace-nowrap rounded px-2 py-1 text-left text-xs hover:bg-border focus-visible:bg-border"
            onClick={() => {
              setElevationMenuOpen(false);
              setOtherUserDraft(elevationTargetUser);
              setOtherUserOpen(true);
              setTimeout(() => otherUserInputRef.current?.focus(), 0);
            }}
          >
            {t("fileElevation.otherUser")}
          </button>
        </div>
      )}
      {otherUserForm}
    </fieldset>
  );
}
