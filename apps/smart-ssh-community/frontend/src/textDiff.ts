// Spec 0019, Abschnitt 4: einfacher zeilenbasierter Diff für die
// Notiz-Änderungs-Vorschau — bewusst kein externes Paket (Notiztexte sind
// kurz, ein voller Text-Diff-Algorithmus wäre hier unnötige Komplexität).
// Klassischer LCS-basierter Zeilen-Diff (dynamische Programmierung über die
// Zeilenzahl beider Texte) — für die hier realistische Textlänge (kurze
// Notizen, keine großen Dateien) unproblematisch in Laufzeit/Speicher.

export type DiffLine = { type: "unchanged" | "added" | "removed"; text: string };

/** Spec 0046, Fund 3: derselbe 256-KB-Cap wie der Lesepfad
 * (`orchestration::MAX_READ_FILE_BYTES`) — deckt den Fall ab, dass eine
 * Seite eine einzelne (oder sehr wenige) riesige Zeile(n) ist, wo
 * [`MAX_DIFF_LINE_PRODUCT`] allein (das an der Zeilen*anzahl* ansetzt)
 * nichts abfangen würde. */
export const MAX_DIFF_INPUT_BYTES = 256 * 1024;

/** spec-reviewer-Fund (Review dieses Schritts): der Byte-Cap allein
 * verhindert den in Spec 0046 §Fund 3 beschriebenen Renderer-Freeze NICHT
 * — die LCS-DP-Tabelle in [`diffLines`] kostet O(Zeilenzahl(before) ×
 * Zeilenzahl(after)), nicht O(Bytes). Ein 250-KB-Inhalt aus lauter
 * kurzen Zeilen (z. B. 5 Byte/Zeile ⇒ ~50.000 Zeilen je Seite) bleibt
 * unter dem Byte-Cap, erzeugt aber eine ~2,5-Milliarden-Zellen-Tabelle —
 * empirisch bestätigter OOM/Freeze-Kandidat. Deshalb zusätzlich ein
 * Zeilenanzahl-*Produkt*-Cap, der direkt an der tatsächlichen
 * DP-Tabellengröße ansetzt: 2000×2000 Zeilen (4 Mio. Zellen) ist für jede
 * realistische Notiz/Datei-Vorschau großzügig, aber weit unter jeder
 * spürbaren Verzögerung. */
export const MAX_DIFF_LINE_PRODUCT = 4_000_000;

function countLines(s: string): number {
  return s.length === 0 ? 0 : s.split("\n").length;
}

/** `true`, wenn `before`/`after` den Diff-Cap überschreiten — entweder weil
 * eine Seite allein schon über [`MAX_DIFF_INPUT_BYTES`] liegt (Byte-Länge,
 * UTF-8, nicht JS-String-Länge, damit dieselbe Grenze wie am Lesepfad
 * gilt) ODER weil das Produkt ihrer Zeilenzahlen über
 * [`MAX_DIFF_LINE_PRODUCT`] liegt (die eigentliche Kostengröße der
 * LCS-Berechnung in [`diffLines`], s. dortiger Kommentar). */
export function isDiffTooLargeToCompute(before: string, after: string): boolean {
  const byteLength = (s: string) => new TextEncoder().encode(s).length;
  if (byteLength(before) > MAX_DIFF_INPUT_BYTES || byteLength(after) > MAX_DIFF_INPUT_BYTES) {
    return true;
  }
  return countLines(before) * countLines(after) > MAX_DIFF_LINE_PRODUCT;
}

/** Wirft, statt eine möglicherweise riesige DP-Tabelle zu allokieren, wenn
 * `before`/`after` [`MAX_DIFF_LINE_PRODUCT`] überschreiten — eine
 * Verteidigungslinie IN dieser Funktion selbst (nicht nur beim Aufrufer
 * `isDiffTooLargeToCompute`/`NoteDiffPreview`), spec-reviewer-Fund: "kein
 * künftiger Aufrufer soll daran vorbeikommen können". Im normalen
 * Ablauf (Aufrufer prüft vorher `isDiffTooLargeToCompute`) wird dieser
 * Zweig nie erreicht, da beide Funktionen dieselbe Konstante nutzen. */
export class DiffTooLargeError extends Error {
  constructor() {
    super("diff input exceeds MAX_DIFF_LINE_PRODUCT");
    this.name = "DiffTooLargeError";
  }
}

export function diffLines(before: string, after: string): DiffLine[] {
  const a = before.length === 0 ? [] : before.split("\n");
  const b = after.length === 0 ? [] : after.split("\n");

  if (a.length * b.length > MAX_DIFF_LINE_PRODUCT) {
    throw new DiffTooLargeError();
  }

  // lcs[i][j] = Länge der längsten gemeinsamen Teilfolge von a[i..]/b[j..].
  const lcs: number[][] = Array.from({ length: a.length + 1 }, () =>
    new Array<number>(b.length + 1).fill(0),
  );
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      lcs[i][j] =
        a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }

  const result: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      result.push({ type: "unchanged", text: a[i] });
      i++;
      j++;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      result.push({ type: "removed", text: a[i] });
      i++;
    } else {
      result.push({ type: "added", text: b[j] });
      j++;
    }
  }
  while (i < a.length) {
    result.push({ type: "removed", text: a[i] });
    i++;
  }
  while (j < b.length) {
    result.push({ type: "added", text: b[j] });
    j++;
  }
  return result;
}

/** Nur die tatsächlich geänderten Zeilen (Spec 0019, Abschnitt 4: "kurz" —
 * unveränderte Zeilen werden weggelassen, nicht der gesamte bestehende Text
 * erneut gezeigt). `null`/leeres `before` (neue Notiz, keine Zielauflösung)
 * liefert den gesamten neuen Text als "added". */
export function shortNoteDiff(before: string | null, after: string): DiffLine[] {
  return diffLines(before ?? "", after).filter((line) => line.type !== "unchanged");
}
