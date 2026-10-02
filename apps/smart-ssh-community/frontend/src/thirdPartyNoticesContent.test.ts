// Spec 0099 (BL-0054), T1/T2: prüft den Inhalt der erzeugten Drittlizenz-
// Datei gegen den heutigen Abhängigkeitsbaum. Läuft nur, wenn
// `public/third-party-notices.txt` existiert (A2.4/A3.1, s. Kommentar in
// thirdPartyNoticesNpmCompleteness.test.ts) — ein Dev-Build ohne
// vorherigen `generate-notices`-Lauf ist kein Fehler dieses Tests.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const FRONTEND_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const NOTICES_PATH = path.join(FRONTEND_DIR, "public/third-party-notices.txt");
const MARKER = "SMART-SSH-THIRD-PARTY-NOTICES-V1";

describe("Drittlizenzen: Inhalt auf dem aktuellen Stand (Spec 0099, T1)", () => {
  const hasOutput = fs.existsSync(NOTICES_PATH);
  const content = hasOutput ? fs.readFileSync(NOTICES_PATH, "utf8") : "";

  it.skipIf(!hasOutput)("beginnt mit der Kennzeile (A1.8)", () => {
    expect(content.startsWith(MARKER)).toBe(true);
  });

  // Je ein Vertreter pro Quelle aus A1.1: (a) Rust, (b) npm-Produktion,
  // (c) devDependency mit ausgeliefertem Code, (d) Schriften. Scheitert,
  // wenn eine der vier Quellen fehlt.
  it.skipIf(!hasOutput)("enthält die Lizenztexte aller vier Quellen aus A1.1", () => {
    expect(content).toContain("Apache License"); // (a) Rust, z. B. tauri
    expect(content).toContain("MIT License"); // (a)/(b)/(c)
    expect(content).toContain("Mozilla Public License"); // (a) Rust, MPL-2.0
    expect(content).toContain("SIL OPEN FONT LICENSE"); // (d) Schriften, OFL-1.1
  });

  it.skipIf(!hasOutput)("listet mindestens tauri, react, tailwindcss und vite als Nutzer", () => {
    expect(content).toMatch(/Verwendet von:.*\btauri \d/);
    expect(content).toMatch(/Verwendet von:.*\breact@\d/);
    expect(content).toMatch(/Verwendet von:.*\btailwindcss@\d/);
    expect(content).toMatch(/Verwendet von:.*\bvite@\d/);
  });

  it.skipIf(!hasOutput)("enthält die Copyright-Zeilen von Barlow und JetBrains Mono (A6)", () => {
    expect(content).toContain("Copyright 2017 The Barlow Project Authors");
    expect(content).toContain("Copyright 2020 The JetBrains Mono Project Authors");
  });

  // T9 (Spec 0101, A1): `libsqlite3-sys`s SQLCipher-Feature und die
  // vendorte OpenSSL-Quelle (`openssl-src`) liegen beide tiefer
  // verschachtelt im jeweiligen Crate, als der generische Rust-Lizenz-Scan
  // greift — ohne die explizite Zuordnung in
  // `collectVendoredLicenseNotices` (generate-third-party-notices.mjs)
  // bleibt das unsichtbar (gemessen: weder "Zetetic"/"SQLCipher" noch der
  // OpenSSL-Lizenztext selbst tauchten vor diesem Fix in der generierten
  // Ausgabe auf).
  it.skipIf(!hasOutput)(
    "enthält die Lizenztexte von SQLCipher und der vendorten OpenSSL-Quelle (Spec 0101, T9)",
    () => {
      expect(content).toContain("SQLCipher (gebündelt in libsqlite3-sys, Spec 0101 A1)");
      expect(content).toContain("Zetetic LLC");
      expect(content).toContain("OpenSSL (vendorte Quelle in openssl-src, Spec 0101 A1)");
    },
  );
});

describe("Drittlizenzen: Hinweisdateien (Spec 0099, T2)", () => {
  const hasOutput = fs.existsSync(NOTICES_PATH);
  const content = hasOutput ? fs.readFileSync(NOTICES_PATH, "utf8") : "";

  it.skipIf(!hasOutput)("enthält den Inhalt von cfg_aliases' NOTICES.md mit Paketnamen", () => {
    expect(content).toContain("cfg_aliases");
    expect(content).toContain("tectonic_cfg_support is licensed under the MIT License");
  });
});
