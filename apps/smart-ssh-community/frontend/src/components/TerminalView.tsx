import { useEffect, useRef } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { commandErrorMessage, openTerminal, terminalInput, terminalResize } from "../api";
import { base64ToBytes, onTerminalOutput } from "../events";

interface TerminalViewProps {
  sessionId: string;
}

/**
 * xterm.js-Wrapper (Spec 0007, Abschnitt 7): `open_terminal` beim Öffnen,
 * `terminal-output`-Events schreiben in xterm, Tastatureingaben gehen über
 * `terminal_input`, Größenänderungen (Fenster-Resize, `ResizeObserver`)
 * über `terminal_resize`.
 */
export function TerminalView({ sessionId }: TerminalViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);

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
      theme: {
        background: "#0c1120",
        foreground: "#dfe3ee",
        cursor: "#6c7cf5",
        cursorAccent: "#0c1120",
        selectionBackground: "rgba(108,124,245,0.35)",
        black: "#0c1120",
        red: "#e5484d",
        green: "#3fb950",
        yellow: "#e0a12c",
        blue: "#6c7cf5",
        magenta: "#a5b0ff",
        cyan: "#5980a6",
        white: "#8b93a8",
        brightBlack: "#4f596e",
        brightRed: "#f0918f",
        brightGreen: "#8fd79a",
        brightYellow: "#f0c46b",
        brightBlue: "#a5b0ff",
        brightMagenta: "#c9d0ff",
        brightCyan: "#9fc0dd",
        brightWhite: "#e6e9f2",
      },
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    let disposed = false;
    let unlistenOutput: (() => void) | undefined;
    let resizeObserver: ResizeObserver | undefined;

    if (containerRef.current) {
      term.open(containerRef.current);
      fitAddon.fit();
    }

    const dataSubscription = term.onData((data) => {
      terminalInput(sessionId, new TextEncoder().encode(data)).catch((err) =>
        console.error(commandErrorMessage(err)),
      );
    });

    openTerminal(sessionId)
      .then(() => terminalResize(sessionId, term.cols, term.rows))
      .catch((err) => {
        term.writeln(`\r\n[Terminal konnte nicht geöffnet werden: ${commandErrorMessage(err)}]`);
      });

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
      dataSubscription.dispose();
      resizeObserver?.disconnect();
      unlistenOutput?.();
      term.dispose();
    };
  }, [sessionId]);

  return <div ref={containerRef} className="h-full w-full overflow-hidden" />;
}
