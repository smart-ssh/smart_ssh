// Issue #51: Aufbereitung des Schritt-Protokolls.
import { describe, expect, it } from "vitest";
import {
  formatDuration,
  formatStepLogText,
  groupByHop,
  recordDetails,
  stepDetails,
  stepLabel,
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
    ).toBe("failed: HOST_KEY_CHANGED");
  });

  it("shows a translated reason for a failed step and the raw code in the details", () => {
    const failed = record({
      step: { kind: "tcpConnect", address: null, port: 22 },
      status: { state: "failed", code: "SSH_TCP_TIMEOUT" },
    });
    expect(stepStatusText(t, failed)).toBe("failed: TCP connection timed out");
    expect(stepStatusText(t, failed)).not.toContain("SSH_TCP_TIMEOUT");
    expect(recordDetails(t, failed)).toBe(":22 · SSH_TCP_TIMEOUT");
  });

  it("labels a skipped DNS step as IP address without lookup", () => {
    const skipped = record({
      step: { kind: "dnsResolution", host: "192.0.2.1", port: 22, addresses: ["192.0.2.1"] },
      status: { state: "skipped" },
    });
    expect(stepStatusText(t, skipped)).toBe("skipped");
    expect(stepLabel(t, skipped.step, "skipped")).toBe("IP address – no lookup needed");
    expect(formatStepLogText(t, [skipped])).toContain("– IP address – no lookup needed — skipped");
    expect(stepLabel(t, skipped.step, "ok")).toBe("DNS resolution");
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
    expect(single).toBe("✗ TCP connection — failed: Connection refused (5 ms) — 192.0.2.1:22 · SSH_CONNECTION_REFUSED");

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
