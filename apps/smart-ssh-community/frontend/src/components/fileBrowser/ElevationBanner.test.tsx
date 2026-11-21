import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ElevationBanner } from "./ElevationBanner";

describe("ElevationBanner", () => {
  it("shows the server logging addition only when server logging is active", () => {
    const { rerender } = render(
      <ElevationBanner elevatedUser="root" warningPath={null} serverLogging onToggleOff={() => {}} />,
    );
    expect(screen.getByRole("alert").textContent).toContain("fileElevation.bannerLogged");
    rerender(
      <ElevationBanner elevatedUser="root" warningPath={null} serverLogging={false} onToggleOff={() => {}} />,
    );
    expect(screen.getByRole("alert").textContent).not.toContain("fileElevation.bannerLogged");
  });

  it("keeps the toggle-off button name and the banner text (notice heading and collapse: ElevationNotice.test.tsx)", async () => {
    const onToggleOff = vi.fn();
    render(<ElevationBanner elevatedUser="root" warningPath={null} onToggleOff={onToggleOff} />);
    fireEvent.click(screen.getByRole("button", { name: "fileElevation.toggleOff" }));
    expect(onToggleOff).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("alert").textContent).toContain("fileElevation.banner");
  });
});
