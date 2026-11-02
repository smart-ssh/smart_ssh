import { useState } from "react";
import { useTranslation } from "react-i18next";
import { type RemoteEntryDto } from "../../types";
import { Button, DialogFrame, Field, FIELD_CONTROL_CLASS } from "../ui";
import { TECHNICAL_INPUT_PROPS } from "../../technicalInputProps";

/** Reihen-Reihenfolge der chmod-Checkbox-Matrix: owner/group/other × je
 * einer Bit-Maske pro Spalte (r/w/x), spiegelt exakt `RemoteEntry::
 * permissions` (Spec 0020: "reine Unix-Rechte-Bits, `0o755`-Stil"). */
const CHMOD_ROWS: { key: "owner" | "group" | "other"; read: number; write: number; execute: number }[] = [
  { key: "owner", read: 0o400, write: 0o200, execute: 0o100 },
  { key: "group", read: 0o040, write: 0o020, execute: 0o010 },
  { key: "other", read: 0o004, write: 0o002, execute: 0o001 },
];

/** Spec 0054, Teil 3: chmod-Dialog — Checkbox-Matrix (owner/group/other ×
 * r/w/x) UND eine numerische Eingabe, bidirektional synchron über
 * gemeinsamen `mode`-State (jede Checkbox toggelt ein einzelnes Bit per
 * XOR, die numerische Eingabe parst die ganze dreistellige Oktalzahl neu —
 * beide schreiben in denselben State, es gibt keine zwei Quellen der
 * Wahrheit). "Optional rekursiv... mit deutlicher Kennzeichnung, weil
 * mächtig" (Spec) nur bei Ordnern sichtbar, mit auffälliger Warnfarbe. */
export function ChmodDialog({
  entry,
  modeNote,
  onCancel,
  onConfirm,
}: {
  entry: RemoteEntryDto;
  /** Spec 0067, A5: im erhöhten Modus, z. B. „Die Rechte werden als root
   * gesetzt.“ */
  modeNote: string | null;
  onCancel: () => void;
  onConfirm: (mode: number, recursive: boolean, applySpecialBits: boolean) => void;
}) {
  const { t } = useTranslation();
  const [mode, setMode] = useState(entry.permissionsOctal);
  // Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets): mit nur 3
  // Ziffern (`maxLength={3}`, `/^[0-7]{1,3}$/`) ließ sich eine Datei mit
  // gesetztem setuid/setgid/sticky-Bit (`permissionsOctal` trägt `&
  // 0o7777`, also auch diese "vierte Ziffer") über die numerische Eingabe
  // nicht mehr KORREKT bearbeiten: eine erneute Eingabe von z. B. "755" auf
  // eine zuvor `4755` (setuid) gesetzte Datei löschte das setuid-Bit still
  // — die Checkbox-Matrix (togglet einzelne Bits per XOR) war davon nicht
  // betroffen, zwei inkonsistente Pfade zum selben `mode`. Durchgehend
  // 4-stellig (führende Null bei "gewöhnlichen" Rechten, z. B. "0644")
  // macht die vierte Ziffer immer sichtbar und bearbeitbar, statt sie
  // implizit wegzulassen.
  const [numericInput, setNumericInput] = useState(mode.toString(8).padStart(4, "0"));
  const [recursive, setRecursive] = useState(false);

  const applyMode = (next: number) => {
    setMode(next);
    setNumericInput(next.toString(8).padStart(4, "0"));
  };

  const toggleBit = (bit: number) => applyMode(mode ^ bit);

  // Issue #624: a recursive apply only touches folders' special bits when
  // the user changed them here.
  const specialChanged = (mode & 0o7000) !== (entry.permissionsOctal & 0o7000);
  const fileBits = mode & 0o777;
  const folderBits = fileBits | ((fileBits & 0o444) >> 2);

  const handleNumericChange = (value: string) => {
    setNumericInput(value);
    if (/^[0-7]{1,4}$/.test(value)) {
      setMode(Number.parseInt(value, 8));
    }
  };

  return (
    <DialogFrame
      layer="sessionTab"
      size="sm"
      title={t("fileBrowser.chmod.title")}
      onClose={onCancel}
      onSubmit={(e) => {
        e.preventDefault();
        onConfirm(mode, recursive, specialChanged);
      }}
      actions={
        <>
          <Button onClick={onCancel}>{t("fileBrowser.cancel")}</Button>
          <Button type="submit" variant="primary">
            {t("fileBrowser.apply")}
          </Button>
        </>
      }
    >
      <p className="mb-3 font-mono text-xs break-all text-fg-subtle">{entry.path}</p>
      {modeNote && (
        <p className="mb-3 border border-warning/60 bg-warning-subtle/40 px-2 py-1 text-xs text-warning-fg-soft">
          ⚠ {modeNote}
        </p>
      )}

      <table className="mb-3 w-full text-sm text-fg-muted">
        <thead>
          <tr className="text-fg-subtle">
            <th className="text-left font-normal">
              <span className="sr-only">{t("fileBrowser.chmodClassColumn")}</span>
            </th>
            <th className="font-normal">{t("fileBrowser.chmod.read")}</th>
            <th className="font-normal">{t("fileBrowser.chmod.write")}</th>
            <th className="font-normal">{t("fileBrowser.chmod.execute")}</th>
          </tr>
        </thead>
        <tbody>
          {CHMOD_ROWS.map((row) => (
            <tr key={row.key}>
              <td>{t(`fileBrowser.chmod.${row.key}`)}</td>
              {(
                [
                  [row.read, "read"],
                  [row.write, "write"],
                  [row.execute, "execute"],
                ] as const
              ).map(([bit, key]) => (
                <td key={bit} className="text-center">
                  <input
                    type="checkbox"
                    checked={(mode & bit) !== 0}
                    onChange={() => toggleBit(bit)}
                    aria-label={`${t(`fileBrowser.chmod.${row.key}`)}: ${t(`fileBrowser.chmod.${key}`)}`}
                    className="focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
                  />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>

      <fieldset className="mb-3 text-sm text-fg-muted">
        <legend className="sr-only">{t("fileBrowser.chmod.special")}</legend>
        <div className="flex flex-wrap gap-x-4 gap-y-1">
          {(
            [
              [0o4000, "setuid"],
              [0o2000, "setgid"],
              [0o1000, "sticky"],
            ] as const
          ).map(([bit, key]) => (
            <label key={key} className="flex items-center gap-1">
              <input
                type="checkbox"
                checked={(mode & bit) !== 0}
                onChange={() => toggleBit(bit)}
                className="focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
              />
              {t(`fileBrowser.chmod.${key}`)}
            </label>
          ))}
        </div>
        <p className="mt-1 text-xs text-fg-subtle">{t("fileBrowser.chmod.specialHint")}</p>
      </fieldset>

      <Field label={t("fileBrowser.chmod.numeric")} className="mb-3">
        {(control) => (
          <input
            {...TECHNICAL_INPUT_PROPS}
            {...control}
            value={numericInput}
            onChange={(e) => handleNumericChange(e.target.value)}
            maxLength={4}
            className={`${FIELD_CONTROL_CLASS} w-24 font-mono`}
          />
        )}
      </Field>

      {entry.isDir && (
        <label className="flex items-start gap-2 text-sm text-warning-fg-soft">
          <input
            type="checkbox"
            checked={recursive}
            onChange={(e) => setRecursive(e.target.checked)}
            className="mt-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
          />
          <span>
            <strong>{t("fileBrowser.chmod.recursive")}</strong>{" "}
            {t("fileBrowser.chmod.recursiveWarning")}
          </span>
        </label>
      )}
      {entry.isDir && recursive && (
        <p className="mt-2 text-xs text-fg-muted">
          {t("fileBrowser.chmod.recursiveSummary", {
            folders: folderBits.toString(8).padStart(3, "0"),
            files: fileBits.toString(8).padStart(3, "0"),
          })}
        </p>
      )}
    </DialogFrame>
  );
}
