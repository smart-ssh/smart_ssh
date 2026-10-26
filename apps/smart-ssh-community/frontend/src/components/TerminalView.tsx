import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { commandErrorMessage, openTerminal, terminalInput, terminalResize } from "../api";
import { base64ToBytes, onTerminalOutput } from "../events";
import { notifyMissingStartDirectory } from "../startDirectory";
import { useMediaQuery } from "../hooks/useMediaQuery";
import { useSoftKeyboardViewport } from "../hooks/useSoftKeyboardViewport";
import { detectFallbackPlatform } from "../titlebarLayout";
import { applyModifiers, keySequence, type TerminalKeyId } from "../terminalKeys";
import { TerminalKeyRow } from "./TerminalKeyRow";

/** xterm theme built from the `term-*` colour tokens (theme/dark.css). */
function readTerminalTheme(): ITheme {
  const style = getComputedStyle(document.documentElement);
  const v = (name: string) => style.getPropertyValue(`--term-${name}`).trim();
  return {
    background: v("bg"),
    foreground: v("fg"),
    cursor: v("cursor"),
    cursorAccent: v("cursor-accent"),
    selectionBackground: v("selection"),
    black: v("ansi-black"),
    red: v("ansi-red"),
    green: v("ansi-green"),
    yellow: v("ansi-yellow"),
    blue: v("ansi-blue"),
    magenta: v("ansi-magenta"),
    cyan: v("ansi-cyan"),
    white: v("ansi-white"),
    brightBlack: v("ansi-bright-black"),
    brightRed: v("ansi-bright-red"),
    brightGreen: v("ansi-bright-green"),
    brightYellow: v("ansi-bright-yellow"),
    brightBlue: v("ansi-bright-blue"),
    brightMagenta: v("ansi-bright-magenta"),
    brightCyan: v("ansi-bright-cyan"),
    brightWhite: v("ansi-bright-white"),
  };
}

/** Issue #355: Verbindungszustand, den `SessionView` dem Terminal meldet. */
export type TerminalLink = "live" | "lost" | "reconnecting";

interface TerminalViewProps {
  sessionId: string;
  link?: TerminalLink;
  /** Taste R im toten Terminal. */
  onReconnect?: () => void;
}

/**
 * xterm.js-Wrapper (Spec 0007, Abschnitt 7): `open_terminal` beim Öffnen,
 * `terminal-output`-Events schreiben in xterm, Tastatureingaben gehen über
 * `terminal_input`, Größenänderungen (Fenster-Resize, `ResizeObserver`)
 * über `terminal_resize`.
 */
export function TerminalView({ sessionId, link = "live", onReconnect }: TerminalViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const { t } = useTranslation();
  const termRef = useRef<XTerm | null>(null);
  const startShellRef = useRef<() => void>(() => {});
  const linkRef = useRef(link);
  const onReconnectRef = useRef(onReconnect);
  const lostShownRef = useRef(false);
  // Issue #678: touch key row and sticky one-shot Ctrl/Alt (Spec 0114).
  const coarse = useMediaQuery("(pointer: coarse)");
  const showKeyRow = coarse || detectFallbackPlatform() === "android";
  useSoftKeyboardViewport(showKeyRow);
  const [mods, setMods] = useState({ ctrl: false, alt: false });
  const modsRef = useRef(mods);
  modsRef.current = mods;
  const sendRef = useRef<(data: string) => void>(() => {});
  linkRef.current = link;
  onReconnectRef.current = onReconnect;

  useEffect(() => {
    // xterm.js liest weder CSS-Custom-Properties noch die `font-mono`-
    // Tailwind-Utility — Schrift und Farben müssen hier explizit dieselben
    // Werte wie das Theme in `index.css` bekommen (Design-Import, Abschnitt
    // 1b: Terminal-Panel). `fontFamily` fehlte bisher komplett (xterm fiel
    // auf seinen eigenen Mono-Stack zurück, nicht JetBrains Mono).
    const term = new XTerm({
      convertEol: true,
      fontSize: 13,
      fontFamily: "'JetBrains Mono', ui-monospace, SFMono-Regular, monospace",
      theme: readTerminalTheme(),
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    // A theme switch changes the `term-*` variables on the root element;
    // re-read them so open terminals follow it live.
    const themeObserver = new MutationObserver(() => {
      term.options.theme = readTerminalTheme();
    });
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme", "class", "style"],
    });

    let disposed = false;
    let unlistenOutput: (() => void) | undefined;
    let resizeObserver: ResizeObserver | undefined;

    if (containerRef.current) {
      term.open(containerRef.current);
      fitAddon.fit();
    }

    const send = (raw: string) => {
      // Issue #355: im toten Terminal geht nichts an den Server; nur R
      // (oder r) löst den Reconnect aus.
      if (linkRef.current !== "live") {
        if (raw === "r" || raw === "R") onReconnectRef.current?.();
        return;
      }
      terminalInput(sessionId, new TextEncoder().encode(raw)).catch((err) =>
        console.error(commandErrorMessage(err)),
      );
    };
    sendRef.current = send;
    const dataSubscription = term.onData((data) => {
      // Issue #678: a pending sticky Ctrl/Alt applies to this key, then clears.
      const m = modsRef.current;
      if (m.ctrl || m.alt) {
        modsRef.current = { ctrl: false, alt: false };
        setMods(modsRef.current);
        send(applyModifiers(data, m));
        return;
      }
      send(data);
    });

    const startShell = () => {
      openTerminal(sessionId)
        .then((start) => {
          notifyMissingStartDirectory(t, start?.missingDirectory ?? null);
          return terminalResize(sessionId, term.cols, term.rows);
        })
        .catch((err) => {
          term.writeln(
            `\r\n[${t("session.terminalOpenFailed", { error: commandErrorMessage(err) })}]`,
          );
        });
    };
    startShellRef.current = startShell;
    termRef.current = term;
    startShell();

    onTerminalOutput((event) => {
      if (event.sessionId !== sessionId || disposed) return;
      term.write(base64ToBytes(event.data));
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
      } else {
        unlistenOutput = unlisten;
      }
    });

    if (containerRef.current) {
      resizeObserver = new ResizeObserver(() => {
        fitAddon.fit();
        terminalResize(sessionId, term.cols, term.rows).catch(() => {
          // Session evtl. schon getrennt — beim nächsten Resize/Reconnect
          // greift es wieder, kein Grund für eine sichtbare Fehlermeldung.
        });
      });
      resizeObserver.observe(containerRef.current);
    }

    return () => {
      disposed = true;
      termRef.current = null;
      dataSubscription.dispose();
      resizeObserver?.disconnect();
      unlistenOutput?.();
      themeObserver.disconnect();
      term.dispose();
    };
    // `t` ist bewusst keine Abhängigkeit: ein Sprachwechsel soll das
    // Terminal nicht neu öffnen.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  // Issue #355: Hinweiszeile beim Verlust, Trennlinie und neue Shell nach
  // dem Reconnect — der alte Verlauf bleibt stehen.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    if ((link === "lost" || link === "reconnecting") && !lostShownRef.current) {
      lostShownRef.current = true;
      term.writeln(`\r\n${t("session.connectionLostPressR")}`);
    } else if (link === "live" && lostShownRef.current) {
      lostShownRef.current = false;
      term.writeln(`\r\n${t("session.reconnectedSeparator")}`);
      startShellRef.current();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [link]);

  const onRowKey = useCallback((key: TerminalKeyId) => {
    const m = modsRef.current;
    modsRef.current = { ctrl: false, alt: false };
    setMods(modsRef.current);
    const appCursor = termRef.current?.modes?.applicationCursorKeysMode ?? false;
    sendRef.current(keySequence(key, m, appCursor));
    termRef.current?.focus();
  }, []);

  return (
    <div className="flex h-full w-full flex-col">
      <div ref={containerRef} className="min-h-0 flex-1 overflow-hidden" />
      {showKeyRow && (
        <TerminalKeyRow
          ctrl={mods.ctrl}
          alt={mods.alt}
          onToggleCtrl={() => setMods((m) => ({ ...m, ctrl: !m.ctrl }))}
          onToggleAlt={() => setMods((m) => ({ ...m, alt: !m.alt }))}
          onKey={onRowKey}
        />
      )}
    </div>
  );
}
