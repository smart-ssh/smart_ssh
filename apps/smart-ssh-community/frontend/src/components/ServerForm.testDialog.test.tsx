// Issue #633: "Test connection" opens a dialog with the live step list.
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { cancelConnect, getServer, testConnection } from "../api";
import { testI18n } from "../testI18n";
import type {
  ConnectProgressEvent,
  ConnectStepSnapshot,
  ServerDto,
  TestConnectionReport,
} from "../types";
import { ServerForm } from "./ServerForm";

let progressHandler: (e: ConnectProgressEvent) => void = () => {};

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) =>
    typeof err === "object" && err !== null && "message" in err
      ? String((err as { message: unknown }).message)
      : String(err),
  commandErrorCode: (err: unknown) =>
    typeof err === "object" && err !== null && "code" in err
      ? ((err as { code: string | null }).code ?? null)
      : null,
  cancelConnect: vi.fn(() => Promise.resolve()),
  clearServerSudoPassword: vi.fn(),
  convertIdentityFileToKeychain: vi.fn(),
  createServer: vi.fn(),
  deleteServer: vi.fn(),
  getServer: vi.fn(),
  inspectKeyFile: vi.fn(),
  listStoredHostKeys: vi.fn(() => Promise.resolve([])),
  largeNoteDialogThresholdChars: vi.fn(() => Promise.resolve(100000)),
  previewEffectiveNotes: vi.fn(() => Promise.resolve("")),
  requestNoteShrink: vi.fn(),
  testConnection: vi.fn(),
  trustHostKey: vi.fn(),
  updateLocalServerNotes: vi.fn(),
  updateLocalServerTags: vi.fn(),
  updateServer: vi.fn(),
}));

vi.mock("../events", () => ({
  onNoteShrinkSucceeded: vi.fn(() => Promise.resolve(() => {})),
  onConnectProgress: vi.fn((h: (e: ConnectProgressEvent) => void) => {
    progressHandler = h;
    return Promise.resolve(() => {});
  }),
}));

vi.mock("../fileDialog", () => ({
  pickAndReadTextFile: vi.fn(() => Promise.resolve(null)),
  pickFilePath: vi.fn(() => Promise.resolve(null)),
}));

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
}));

const SERVER_ID = "11111111-1111-4111-8111-111111111111";

const server: ServerDto = {
  id: SERVER_ID,
  name: "web-01",
  host: "example.invalid",
  port: 22,
  username: "deploy",
  groupId: null,
  tags: [],
  authKind: "agent",
  identityFilePath: null,
  jumpHost: null,
  notes: "",
  hasSudoPassword: false,
  sudoPasswordUnknown: false,
  isLocal: false,
  postIngestPolicy: "balanced",
  aiInjectionCheckEnabled: false,
  sftpServerPath: null,
};

const step = (
  kind: ConnectStepSnapshot["kind"],
  status: ConnectStepSnapshot["status"],
  errorCode: string | null = null,
): ConnectStepSnapshot => ({
  kind,
  hopIndex: 0,
  hop: "deploy@example.invalid:22",
  status,
  errorCode,
  detail: null,
});

async function startTest(): Promise<string> {
  vi.mocked(getServer).mockResolvedValue(server);
  render(
    <I18nextProvider i18n={testI18n}>
      <ServerForm
        serverId={SERVER_ID}
        defaultGroupId={null}
        allGroups={[]}
        allServers={[]}
        onSaved={vi.fn()}
        onDeleted={vi.fn()}
      />
    </I18nextProvider>,
  );
  expect(await screen.findByDisplayValue("web-01")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("tab", { name: "Verbindung" }));
  fireEvent.click(screen.getByRole("button", { name: "Verbindung testen" }));
  await waitFor(() => expect(testConnection).toHaveBeenCalled());
  return vi.mocked(testConnection).mock.calls[0][1];
}


function emit(attemptId: string, steps: ConnectStepSnapshot[]) {
  act(() => progressHandler({ attemptId, serverId: SERVER_ID, attempt: 1, steps }));
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("ServerForm — test connection dialog (issue #633)", () => {
  it("shows steps as events arrive and Cancel calls cancel_connect", async () => {
    vi.mocked(testConnection).mockImplementation(() => new Promise(() => {}));
    const attemptId = await startTest();
    const dialog = await screen.findByRole("dialog");

    emit(attemptId, [step("tcpConnect", "ok"), step("handshake", "running")]);
    expect(within(dialog).getAllByRole("listitem")).toHaveLength(2);
    // Events of another attempt are ignored.
    emit("other", [step("tcpConnect", "ok")]);
    expect(within(dialog).getAllByRole("listitem")).toHaveLength(2);

    fireEvent.click(within(dialog).getByRole("button", { name: "Abbrechen" }));
    expect(cancelConnect).toHaveBeenCalledWith(attemptId);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("Escape cancels a running test", async () => {
    vi.mocked(testConnection).mockImplementation(() => new Promise(() => {}));
    const attemptId = await startTest();
    const dialog = await screen.findByRole("dialog");
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(cancelConnect).toHaveBeenCalledWith(attemptId);
  });

  it("shows earlier steps done and the failed auth step, and Close leaves the badge", async () => {
    let resolveTest: (r: TestConnectionReport) => void = () => {};
    vi.mocked(testConnection).mockImplementation(
      () => new Promise<TestConnectionReport>((r) => (resolveTest = r)),
    );
    const attemptId = await startTest();
    const dialog = await screen.findByRole("dialog");

    const steps = [
      step("tcpConnect", "ok"),
      step("handshake", "ok"),
      step("authentication", "failed", "SSH_AUTH_FAILED"),
    ];
    emit(attemptId, steps);
    await act(async () => resolveTest({ kind: "authenticationFailed", steps: [] }));

    const items = within(dialog).getAllByRole("listitem");
    expect(items.map((li) => li.getAttribute("data-status"))).toEqual(["ok", "ok", "failed"]);
    expect(items[2]).toHaveAttribute("data-failed", "true");
    expect(within(dialog).queryByRole("button", { name: "Abbrechen" })).not.toBeInTheDocument();
    expect(within(dialog).getByTestId("test-connection-result")).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: "Schließen" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(cancelConnect).not.toHaveBeenCalled();
    expect(screen.getByText(/Anmeldung abgelehnt/)).toBeInTheDocument();
  });

  it("a cancelled test ends without an error", async () => {
    let rejectTest: (e: unknown) => void = () => {};
    vi.mocked(testConnection).mockImplementation(
      () => new Promise<TestConnectionReport>((_, rej) => (rejectTest = rej)),
    );
    await startTest();
    await screen.findByRole("dialog");
    fireEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    await act(async () =>
      rejectTest({ message: "Verbindungsaufbau abgebrochen", code: "SSH_CONNECT_CANCELLED" }),
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByText(/abgebrochen/)).not.toBeInTheDocument();
  });
});
