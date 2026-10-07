// Issue #51: reine Aufbereitung des Schritt-Protokolls eines
// Verbindungsversuchs (Test und Verbinden) — Gruppierung je Hop, Texte je
// Schritt und die Textfassung für "Kopieren". Die Anzeige selbst steht in
// `components/ConnectStepLog.tsx`.
//
// Das Protokoll ist flüchtig: Es lebt nur im Zustand der Komponente und
// wird nirgends gespeichert oder geloggt.
import type { ConnectStep, ConnectStepRecord } from "./types";

type T = (key: string, options?: Record<string, unknown>) => string;

export interface HopGroup {
  hopIndex: number;
  hop: string;
  steps: ConnectStepRecord[];
}

/** Gruppiert die Schritte nach Hop, in der Reihenfolge des Auftretens. */
export function groupByHop(steps: ConnectStepRecord[]): HopGroup[] {
  const groups: HopGroup[] = [];
  for (const step of steps) {
    let group = groups.find((g) => g.hopIndex === step.hopIndex);
    if (!group) {
      group = { hopIndex: step.hopIndex, hop: step.hop, steps: [] };
      groups.push(group);
    }
    group.steps.push(step);
  }
  return groups;
}

/** Bezeichnung eines Schritts ("DNS-Auflösung", …). */
export function stepLabel(t: T, step: ConnectStep): string {
  return t(`connectLog.step.${step.kind}`);
}

/** Die Parameter eines Schritts als kurze Zeile; leer, wenn es keine gibt. */
export function stepDetails(t: T, step: ConnectStep): string {
  const parts: string[] = [];
  switch (step.kind) {
    case "dnsResolution":
      parts.push(`${step.host}:${step.port}`);
      if (step.addresses.length > 0) {
        parts.push(t("connectLog.detail.addresses", { addresses: step.addresses.join(", ") }));
      }
      break;
    case "tcpConnect":
      parts.push(step.address ? `${formatHost(step.address)}:${step.port}` : `:${step.port}`);
      break;
    case "tunnelOpen":
      parts.push(`${step.host}:${step.port}`);
      break;
    case "handshake":
      if (step.serverVersion) parts.push(step.serverVersion);
      if (step.kex) parts.push(t("connectLog.detail.kex", { value: step.kex }));
      if (step.hostKeyAlgorithm) {
        parts.push(t("connectLog.detail.hostKeyAlgorithm", { value: step.hostKeyAlgorithm }));
      }
      if (step.cipher) parts.push(t("connectLog.detail.cipher", { value: step.cipher }));
      if (step.mac) parts.push(t("connectLog.detail.mac", { value: step.mac }));
      break;
    case "hostKeyCheck":
      if (step.keyType) parts.push(step.keyType);
      if (step.fingerprint) parts.push(step.fingerprint);
      if (step.result) parts.push(t(`connectLog.hostKeyResult.${step.result}`));
      break;
    case "authentication":
      parts.push(t(`connectLog.authMethod.${step.method}`));
      if (step.partialSuccess) parts.push(t("connectLog.detail.partialSuccess"));
      if (step.remainingMethods.length > 0) {
        parts.push(
          t("connectLog.detail.remainingMethods", { methods: step.remainingMethods.join(", ") }),
        );
      }
      break;
    case "sessionReady":
      break;
  }
  return parts.join(" · ");
}

/** IPv6-Adressen in Klammern, damit der Port lesbar bleibt. */
function formatHost(address: string): string {
  return address.includes(":") ? `[${address}]` : address;
}

/** Ergebnis eines Schritts als kurzer Text. */
export function stepStatusText(t: T, record: ConnectStepRecord): string {
  switch (record.status.state) {
    case "ok":
      return t("connectLog.status.ok");
    case "running":
      return t("connectLog.status.running");
    case "failed":
      return t("connectLog.status.failed", { code: record.status.code });
  }
}

/** Dauer in ms bzw. s. */
export function formatDuration(durationMs: number | null): string {
  if (durationMs === null) return "–";
  if (durationMs < 1000) return `${durationMs} ms`;
  return `${(durationMs / 1000).toFixed(1)} s`;
}

/** Textfassung für die Zwischenablage. Hop-Überschriften nur bei mehr als
 * einem Hop, wie in der Anzeige. */
export function formatStepLogText(t: T, steps: ConnectStepRecord[]): string {
  const groups = groupByHop(steps);
  const multiHop = groups.length > 1;
  const lines: string[] = [];
  for (const group of groups) {
    if (multiHop) {
      lines.push(t("connectLog.hop", { index: group.hopIndex + 1, hop: group.hop }));
    }
    for (const record of group.steps) {
      const details = stepDetails(t, record.step);
      const marker = record.status.state === "failed" ? "✗" : record.status.state === "ok" ? "✓" : "…";
      lines.push(
        `${multiHop ? "  " : ""}${marker} ${stepLabel(t, record.step)} — ${stepStatusText(t, record)} (${formatDuration(record.durationMs)})${details ? ` — ${details}` : ""}`,
      );
    }
  }
  return lines.join("\n");
}
