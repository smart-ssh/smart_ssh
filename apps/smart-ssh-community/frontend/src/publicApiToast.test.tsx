// Issue #1166: showToast (public-api) and ToastHost (test-support) work together.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { act } from "react";
import { showToast } from "./public-api";
import { ToastHost } from "./test-support";

describe("public toast surface", () => {
  it("renders a toast raised through the public API in the test-support host", async () => {
    render(<ToastHost />);
    act(() => showToast({ kind: "error", message: "ext boom" }));
    expect((await screen.findByRole("status")).textContent).toContain("ext boom");
  });
});
