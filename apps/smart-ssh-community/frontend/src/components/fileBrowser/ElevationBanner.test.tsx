import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

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
});
