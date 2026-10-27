// Issue #882: the exported start sequence for embedding apps (Spec 0038, section 4).
import { screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getStartupState } from "./api";
import { installContextMenuSuppression } from "./contextMenu";
import { initI18n } from "./i18n";
import type { StartupStateDto } from "./types";
import { bootstrap } from "./bootstrap";

vi.mock("./api", () => ({ getStartupState: vi.fn() }));
vi.mock("./i18n", () => ({ initI18n: vi.fn(() => Promise.resolve()) }));
vi.mock("./contextMenu", () => ({ installContextMenuSuppression: vi.fn() }));
vi.mock("./theme", () => ({
  initTheme: vi.fn(),
  applyStoredTheme: vi.fn(() => Promise.resolve()),
}));
vi.mock("./titlebarLayout", () => ({ detectFallbackPlatform: () => "android" }));
vi.mock("./App.tsx", () => ({ default: () => <div>THE-APP</div> }));
vi.mock("./components/AppRoot.tsx", () => ({
  AppRoot: ({ children }: { children: ReactNode }) => <>{children}</>,
}));
// The real gate is tested in StartupGate.test.tsx; here it only needs to
// render its children when unlocked.
vi.mock("./components/StartupGate.tsx", () => ({
  StartupGate: ({
    initialState,
    children,
  }: {
    initialState: StartupStateDto | null;
    children: ReactNode;
  }) => (initialState?.screen === "unlocked" ? <>{children}</> : <div>LOCKED</div>),
}));

function state(screen: StartupStateDto["screen"]): StartupStateDto {
  return {
    screen,
    mode: "password",
    offersStartOver: false,
    failedUnlockAttempts: 0,
    language: "de",
  };
}

beforeEach(() => {
  document.body.innerHTML = '<div id="root"></div>';
  vi.mocked(getStartupState).mockResolvedValue(state("unlocked"));
});

afterEach(() => {
  vi.unstubAllEnvs();
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

describe("bootstrap", () => {
  it("sets the platform attribute", async () => {
    await bootstrap();
    expect(document.documentElement.dataset.platform).toBe("android");
    await screen.findByText("THE-APP");
  });

  it("installs the context-menu suppression only in production", async () => {
    vi.stubEnv("PROD", true);
    await bootstrap();
    expect(installContextMenuSuppression).toHaveBeenCalledTimes(1);
    vi.mocked(installContextMenuSuppression).mockClear();
    document.body.innerHTML = '<div id="root"></div>';
    vi.stubEnv("PROD", false);
    await bootstrap();
    expect(installContextMenuSuppression).not.toHaveBeenCalled();
  });

  it("runs getStartupState, initI18n, beforeRender, then renders", async () => {
    const order: string[] = [];
    vi.mocked(getStartupState).mockImplementation(async () => {
      order.push("state");
      return state("unlock");
    });
    vi.mocked(initI18n).mockImplementation(async (lang?: string) => {
      order.push(`i18n:${lang}`);
    });
    await bootstrap({
      beforeRender: async () => {
        order.push("before");
        expect(document.getElementById("root")!.childElementCount).toBe(0);
      },
    });
    expect(order).toEqual(["state", "i18n:de", "before"]);
    await screen.findByText("LOCKED");
  });

  it("logs a rejecting beforeRender and still renders", async () => {
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    await bootstrap({ beforeRender: () => Promise.reject(new Error("boom")) });
    expect(err).toHaveBeenCalled();
    await screen.findByText("THE-APP");
  });

  it("renders extensions only when unlocked; a throwing one leaves the app", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const Boom = () => {
      throw new Error("ext");
    };
    const extensions = [
      { id: "ok", Component: () => <div>EXT-OK</div> },
      { id: "boom", Component: Boom },
    ];
    vi.mocked(getStartupState).mockResolvedValue(state("unlock"));
    await bootstrap({ extensions });
    await screen.findByText("LOCKED");
    expect(screen.queryByText("EXT-OK")).toBeNull();

    document.body.innerHTML = '<div id="root"></div>';
    vi.mocked(getStartupState).mockResolvedValue(state("unlocked"));
    await bootstrap({ extensions });
    await waitFor(() => {
      expect(screen.getByText("THE-APP")).toBeTruthy();
      expect(screen.getByText("EXT-OK")).toBeTruthy();
    });
  });

  it("passes the App element to wrap and renders the result", async () => {
    const wrap = vi.fn((app: ReactNode) => <section data-testid="wrapped">{app}</section>);
    await bootstrap({ wrap });
    expect(wrap).toHaveBeenCalledTimes(1);
    const wrapped = await screen.findByTestId("wrapped");
    expect(wrapped.textContent).toContain("THE-APP");
  });
});
