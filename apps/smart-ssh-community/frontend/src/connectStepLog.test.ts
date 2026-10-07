// Issue #51: Aufbereitung des Schritt-Protokolls.
import { describe, expect, it } from "vitest";
import {
  formatDuration,
  formatStepLogText,
  groupByHop,
  stepDetails,
  stepStatusText,
} from "./connectStepLog";
import { testI18n } from "./testI18n";
import type { ConnectStepRecord } from "./types";

const t = testI18n.getFixedT("en");

function record(overrides: Partial<ConnectStepRecord>): ConnectStepRecord {
  return {
    hopIndex: 0,
    hop: "deploy@jump.example:22",
    step: { kind: "sessionReady" },
    status: { state: "ok" },
    durationMs: 5,
    ...overrides,
  };
}

describe("connectStepLog", () => {
  it("groups steps per hop in order", () => {
    const groups = groupByHop([
      record({ hopIndex: 0 }),
      record({ hopIndex: 1, hop: "root@target:22" }),
      record({ hopIndex: 1, hop: "root@target:22" }),
    ]);
    expect(groups.map((g) => [g.hopIndex, g.hop, g.steps.length])).toEqual([
      [0, "deploy@jump.example:22", 1],
      [1, "root@target:22", 2],
    ]);
  });

  it("describes handshake, host key and authentication parameters", () => {
    expect(
      stepDetails(t, {
        kind: "handshake",
        serverVersion: "SSH-2.0-OpenSSH_9.6",
        kex: "curve25519-sha256",
        hostKeyAlgorithm: "ssh-ed25519",
        cipher: "chacha20-poly1305@openssh.com",
        mac: "none",
      }),
    ).toBe(
      "SSH-2.0-OpenSSH_9.6 · Key exchange: curve25519-sha256 · Host key: ssh-ed25519 · Cipher: chacha20-poly1305@openssh.com · MAC: none",
    );
    expect(
      stepDetails(t, {
        kind: "hostKeyCheck",
        keyType: "ssh-ed25519",
        fingerprint: "SHA256:abc",
        result: "unknown",
      }),
    ).toBe("ssh-ed25519 · SHA256:abc · unknown");
    expect(
      stepDetails(t, {
        kind: "authentication",
        method: "password",
        remainingMethods: ["publickey", "keyboard-interactive"],
        partialSuccess: false,
      }),
    ).toBe("Password · Server also offers: publickey, keyboard-interactive (not tried)");
    expect(stepDetails(t, { kind: "tcpConnect", address: "2001:db8::1", port: 22 })).toBe(
      "[2001:db8::1]:22",
    );
  });

  it("names the failure code of a failed step", () => {
    expect(
      stepStatusText(t, record({ status: { state: "failed", code: "HOST_KEY_CHANGED" } })),
    ).toBe("failed (HOST_KEY_CHANGED)");
  });

  it("formats durations", () => {
    expect(formatDuration(null)).toBe("–");
    expect(formatDuration(42)).toBe("42 ms");
    expect(formatDuration(1530)).toBe("1.5 s");
  });

  it("copies a text version with hop headings only for multi-hop attempts", () => {
    const single = formatStepLogText(t, [
      record({
        step: { kind: "tcpConnect", address: "192.0.2.1", port: 22 },
        status: { state: "failed", code: "SSH_CONNECTION_REFUSED" },
      }),
    ]);
    expect(single).toBe("✗ TCP connection — failed (SSH_CONNECTION_REFUSED) (5 ms) — 192.0.2.1:22");

    const multi = formatStepLogText(t, [
      record({ hopIndex: 0 }),
      record({ hopIndex: 1, hop: "root@target:22", step: { kind: "tunnelOpen", host: "target", port: 22 } }),
    ]);
    expect(multi.split("\n")).toEqual([
      "Hop 1: deploy@jump.example:22",
      "  ✓ Session established — OK (5 ms)",
      "Hop 2: root@target:22",
      "  ✓ Tunnel via previous hop — OK (5 ms) — target:22",
    ]);
  });
});
