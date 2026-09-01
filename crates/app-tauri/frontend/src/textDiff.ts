// Spec 0019, Abschnitt 4: einfacher zeilenbasierter Diff für die
// Notiz-Änderungs-Vorschau — bewusst kein externes Paket (Notiztexte sind
// kurz, ein voller Text-Diff-Algorithmus wäre hier unnötige Komplexität).
// Klassischer LCS-basierter Zeilen-Diff (dynamische Programmierung über die
// Zeilenzahl beider Texte) — für die hier realistische Textlänge (kurze
// Notizen, keine großen Dateien) unproblematisch in Laufzeit/Speicher.

export type DiffLine = { type: "unchanged" | "added" | "removed"; text: string };

export function diffLines(before: string, after: string): DiffLine[] {
  const a = before.length === 0 ? [] : before.split("\n");
  const b = after.length === 0 ? [] : after.split("\n");

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
