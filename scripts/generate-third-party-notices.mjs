// Spec 0099 (BL-0054), A1: erzeugt eine einzelne Textdatei mit den Lizenzen
// (a) aller Rust-Abhängigkeiten des Workspace (`cargo-about`), (b) aller
// Produktionsabhängigkeiten des Frontends (`npm ls`), (c) der
// devDependencies, deren Code ins Bundle gelangt, und (d) der mitgelieferten
// Schriften (A6).
//
// Bewusst ohne Shebang-Zeile (Issue #118): Die Datei ist nicht ausführbar
// und läuft immer über `node`. Ein Frontend-Test importiert sie; unter
// Windows (CRLF-Checkout) erkennt Vites SSR-Transformation die Shebang-Zeile
// nicht und setzt die gehobenen Importe davor — ein Syntaxfehler.
//
// Aufruf über `npm run generate-notices` im Frontend (package.json) — nicht
// direkt `node scripts/generate-third-party-notices.mjs`: das macht den
// Lauf von genau der Stelle aus reproduzierbar, die CI und Entwicklung
// ohnehin schon für alle anderen Frontend-Schritte benutzen, und braucht
// keine separate Erklärung, welcher Node in welchem PATH gemeint ist.
//
// A1.5 (eine Quelle für die erlaubten Lizenzen): Das Skript liest
// `[licenses] allow = [...]` aus `deny.toml` (Parameter `--licenses`,
// Vorgabe: die echte `deny.toml` im Repo-Wurzelverzeichnis) und baut daraus
// zur Laufzeit ein `about.toml` für `cargo-about` — es gibt keine zweite,
// von Hand gepflegte Liste. Dieselbe geparste Liste prüft auch jedes
// npm-Paket (b, c).
//
// A1.3 (Abbruch ohne Ausgabedatei): Jeder Fehlerpfad läuft über `fail()`
// bzw. eine nicht abgefangene Ausnahme; `main()` löscht die Zieldatei davor
// UND in jedem Fehlerfall danach, damit nie eine veraltete oder
// unvollständige Ausgabe liegen bleibt, die einen Erfolg vortäuscht.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");

// A1.8: feste Kennzeile, an der die App (A4.3) eine echte Ausgabe dieses
// Skripts von einem Ladefehler oder einer zufällig getroffenen anderen
// Antwort (z. B. der SPA-Startseite im Dev-Build) unterscheidet.
// Maßgebliche Quelle der Kennzeile (Issue #7, ADR 0100): die Frontend-Kopie
// (`src/thirdPartyNoticesMarker.ts`) und die Prüfung in
// `.github/workflows/release.yml` werden von
// `src/thirdPartyNoticesMarker.test.ts` gegen genau diese Zeile geprüft.
export const MARKER = "SMART-SSH-THIRD-PARTY-NOTICES-V1";

// A1.6: feste Werkzeugversion, lokal und in der CI identisch. Es gibt
// bewusst nur diese eine Stelle: Sowohl ein lokaler Lauf als auch die
// Community-CI (`.github/workflows/community.yml`,
// `taiki-e/install-action` mit `cargo-about@<Version>`) müssen dieselbe
// Versionszeichenkette verwenden; ein Auseinanderlaufen fällt hier als
// Abbruch auf, nicht erst als stiller Unterschied in der Ausgabe.
const EXPECTED_CARGO_ABOUT_VERSION = "0.9.2";

class GenerationError extends Error {}

function fail(message) {
  throw new GenerationError(message);
}

// Klarstellung Spec 0099 Abschnitt 9 (2026-10-01, Windows): Unter PowerShell
// bricht ein Kindprozess ab, dessen Ausgabe per `encoding: "utf8"`
// mitgeschnitten (nicht per `-o`/`--output-file` umgeleitet) wird, wenn das
// übergeordnete Programm selbst unter PowerShell läuft. Für `cargo about
// generate` lösen wir das über dessen eigene `-o`-Option (s.
// `runCargoAbout`); diese Hilfsfunktion sorgt zusätzlich dafür, dass
// `spawnSync("npm", ...)` unter Windows `npm.cmd` über die Shell findet,
// statt am fehlenden `.exe`/`.cmd`-Suffix zu scheitern. Die Argumentliste
// bleibt in jedem Fall fest (keine Nutzereingabe erreicht die Shell).
function spawnCommand(command, args, options) {
  const useShell = process.platform === "win32";
  return spawnSync(command, args, useShell ? { ...options, shell: true } : options);
}

function parseArgs(argv) {
  const defaults = {
    output: path.join(
      REPO_ROOT,
      "apps/smart-ssh-community/frontend/public/third-party-notices.txt",
    ),
    workspace: path.join(REPO_ROOT, "Cargo.toml"),
    frontend: path.join(REPO_ROOT, "apps/smart-ssh-community/frontend"),
    licenses: path.join(REPO_ROOT, "deny.toml"),
    bundledDevDeps: ["tailwindcss", "vite"],
    fontLicense: path.join(
      REPO_ROOT,
      "apps/smart-ssh-community/frontend/src/assets/fonts/LICENSE-OFL.txt",
    ),
  };
  const out = { ...defaults };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const takeNext = () => {
      i++;
      if (i >= argv.length) fail(`Parameter ${arg} braucht einen Wert.`);
      return argv[i];
    };
    switch (arg) {
      case "--output":
        out.output = path.resolve(takeNext());
        break;
      case "--workspace":
        out.workspace = path.resolve(takeNext());
        break;
      case "--frontend":
        out.frontend = path.resolve(takeNext());
        break;
      case "--licenses":
        out.licenses = path.resolve(takeNext());
        break;
      case "--bundled-dev-deps":
        out.bundledDevDeps = takeNext()
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean);
        break;
      case "--font-license":
        out.fontLicense = path.resolve(takeNext());
        break;
      default:
        fail(`Unbekannter Parameter: ${arg}`);
    }
  }
  return out;
}

// --- A1.5: erlaubte Lizenzen aus deny.toml (oder dem per --licenses
// übergebenen Äquivalent) ---------------------------------------------

function parseAllowList(licensesFilePath) {
  if (!fs.existsSync(licensesFilePath)) {
    fail(`Lizenz-Konfiguration nicht gefunden: ${licensesFilePath}`);
  }
  const content = fs.readFileSync(licensesFilePath, "utf8");
  const match = content.match(/\[licenses\][\s\S]*?allow\s*=\s*\[([\s\S]*?)\]/);
  if (!match) {
    fail(`Kein "[licenses] allow = [...]"-Block in ${licensesFilePath} gefunden.`);
  }
  const entries = [...match[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
  if (entries.length === 0) {
    fail(`Die erlaubte-Lizenzen-Liste in ${licensesFilePath} ist leer.`);
  }
  return entries;
}

// Einfache SPDX-"OR"/"AND"-Auswertung für npm-Lizenzfelder. Die
// Rust-Seite prüft cargo-about selbst (vollständiger SPDX-Ausdrucks-
// Parser über das generierte about.toml); diese Funktion deckt nur die in
// der Praxis bei npm vorkommenden einfachen Ausdrücke ab (einzelne
// Lizenz, "X OR Y" bei dual-lizenzierten Paketen), s. Ist-Stand der Spec.
function isLicenseAllowed(expr, allowList) {
  const allowSet = new Set(allowList);
  const orParts = expr.split(/\s+OR\s+/i);
  return orParts.some((part) => {
    const andParts = part
      .trim()
      .replace(/^\(+|\)+$/g, "")
      .split(/\s+AND\s+/i);
    return andParts.every((atom) => allowSet.has(atom.trim()));
  });
}

function writeGeneratedAboutToml(allowList, tmpDir) {
  const aboutTomlPath = path.join(tmpDir, "about.toml");
  const accepted = allowList.map((l) => `    "${l.replace(/"/g, '\\"')}"`).join(",\n");
  fs.writeFileSync(
    aboutTomlPath,
    `# Erzeugt von scripts/generate-third-party-notices.mjs aus ${path.basename(
      "deny.toml",
    )} — nicht von Hand pflegen (A1.5).\naccepted = [\n${accepted}\n]\nprivate = { ignore = true }\n`,
  );
  return aboutTomlPath;
}

// --- Rust-Seite (cargo-about) ------------------------------------------

function runCargoAbout(workspaceManifest, aboutTomlPath, tmpDir) {
  const versionResult = spawnSync("cargo", ["about", "--version"], { encoding: "utf8" });
  if (versionResult.error) {
    fail(`cargo-about ist nicht verfügbar (PATH prüfen): ${versionResult.error.message}`);
  }
  if (versionResult.status !== 0) {
    fail(
      `"cargo about --version" ist fehlgeschlagen (Rückgabewert ${versionResult.status}): ${versionResult.stderr}`,
    );
  }
  const versionOut = (versionResult.stdout || "").trim();
  if (!versionOut.includes(EXPECTED_CARGO_ABOUT_VERSION)) {
    fail(
      `cargo-about hat die falsche Version ("${versionOut}"), erwartet ${EXPECTED_CARGO_ABOUT_VERSION} (A1.6).`,
    );
  }

  // Klarstellung Spec 0099 Abschnitt 9: Unter PowerShell bricht
  // `cargo about generate` ab, wenn seine Ausgabe über die mitgeschnittene
  // `stdout` des Kindprozesses abgegriffen wird ("should not redirect its
  // output in powershell"). Die Ausgabe geht deshalb plattformunabhängig
  // über `-o`/`--output-file` in eine Datei im ohnehin vorhandenen
  // temporären Verzeichnis, von dort wird sie gelesen.
  const aboutOutputPath = path.join(tmpDir, "cargo-about-output.json");
  const result = spawnSync(
    "cargo",
    [
      "about",
      "generate",
      "--format",
      "json",
      "--all-features",
      "--workspace",
      "-m",
      workspaceManifest,
      "-c",
      aboutTomlPath,
      "-o",
      aboutOutputPath,
    ],
    { encoding: "utf8", maxBuffer: 1024 * 1024 * 128 },
  );
  if (result.error) {
    fail(`"cargo about generate" ließ sich nicht starten: ${result.error.message}`);
  }
  if (result.status !== 0) {
    fail(
      `"cargo about generate" ist fehlgeschlagen (Rückgabewert ${result.status}):\n${result.stderr}`,
    );
  }
  let outputText;
  try {
    outputText = fs.readFileSync(aboutOutputPath, "utf8");
  } catch (err) {
    fail(`Ausgabedatei von "cargo about generate" (${aboutOutputPath}) ließ sich nicht lesen: ${err.message}`);
  }
  try {
    return JSON.parse(outputText);
  } catch (err) {
    fail(`Ausgabe von "cargo about generate --format json" ließ sich nicht lesen: ${err.message}`);
  }
}

function collectRustNotices(aboutJson) {
  const notices = [];
  for (const crate of aboutJson.crates) {
    const dir = path.dirname(crate.package.manifest_path);
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (entry.isFile() && /^notice/i.test(entry.name)) {
        notices.push({
          label: `${crate.package.name} ${crate.package.version} (Rust)`,
          content: fs.readFileSync(path.join(dir, entry.name), "utf8"),
        });
      }
    }
  }
  return notices;
}

// Spec 0101 (BL-0314), A1: `libsqlite3-sys`s Feature
// `bundled-sqlcipher-vendored-openssl` baut SQLCipher und eine vendorte
// OpenSSL-Quelle (`openssl-src`) mit ein — beider Lizenztext liegt aber
// NICHT im Wurzelverzeichnis des jeweiligen Crates (wo der `/^notice/i`-Scan
// oben greift), sondern tiefer verschachtelt. Gemessen: ohne diese
// explizite Zuordnung taucht weder "Zetetic"/"SQLCipher" noch der
// OpenSSL-Lizenztext irgendwo in der generierten Ausgabe auf. Eine feste
// Liste statt eines Auto-Scans, weil es hier nicht um ein Namensmuster
// geht, sondern um je eine konkrete, bekannte Datei in einem konkreten
// Crate.
const VENDORED_LICENSE_FILES = [
  {
    crateName: "libsqlite3-sys",
    relativePath: "sqlcipher/LICENSE",
    label: "SQLCipher (gebündelt in libsqlite3-sys, Spec 0101 A1)",
  },
  {
    crateName: "openssl-src",
    relativePath: "openssl/LICENSE.txt",
    label: "OpenSSL (vendorte Quelle in openssl-src, Spec 0101 A1)",
  },
];

function collectVendoredLicenseNotices(aboutJson) {
  const notices = [];
  const matchedCrateNames = new Set();
  for (const crate of aboutJson.crates) {
    const dir = path.dirname(crate.package.manifest_path);
    for (const entry of VENDORED_LICENSE_FILES) {
      if (crate.package.name !== entry.crateName) continue;
      const filePath = path.join(dir, entry.relativePath);
      if (!fs.existsSync(filePath)) {
        fail(
          `Erwartete gebündelte Lizenzdatei fehlt: ${filePath} (Crate ${crate.package.name} ` +
            `${crate.package.version}, Spec 0101 A1) — Pfad oder Version im Abhängigkeitsbaum ` +
            "geändert?",
        );
      }
      matchedCrateNames.add(entry.crateName);
      notices.push({
        label: `${entry.label} — ${crate.package.name} ${crate.package.version}`,
        content: fs.readFileSync(filePath, "utf8"),
      });
    }
  }
  // Fehlt eines der beiden Crates ganz (z. B. eine künftige Abhängigkeits-
  // Umstellung), bliebe der Lizenztext sonst stillschweigend weg, statt den
  // Lauf rot zu färben (Spec 0101 A1 verlangt beide Texte in jeder
  // Ausgabe).
  for (const entry of VENDORED_LICENSE_FILES) {
    if (!matchedCrateNames.has(entry.crateName)) {
      fail(
        `Crate "${entry.crateName}" nicht im Abhängigkeitsbaum gefunden — sein Lizenztext ` +
          "(Spec 0101 A1) kann nicht eingebettet werden.",
      );
    }
  }
  return notices;
}

// --- npm-Seite -----------------------------------------------------------

function runNpmLs(frontendDir) {
  const result = spawnCommand("npm", ["ls", "--omit=dev", "--all", "--json"], {
    cwd: frontendDir,
    encoding: "utf8",
    maxBuffer: 1024 * 1024 * 64,
  });
  if (result.error) {
    fail(`"npm ls" ließ sich nicht starten (Frontend: ${frontendDir}): ${result.error.message}`);
  }
  // `npm ls` liefert bei ungelösten peer-deps einen Rückgabewert != 0, auch
  // wenn die JSON-Ausgabe selbst vollständig ist — deshalb hier auf
  // vorhandene, parsbare Ausgabe prüfen statt auf den Rückgabewert.
  if (!result.stdout) {
    fail(`"npm ls --omit=dev --all --json" lieferte keine Ausgabe:\n${result.stderr}`);
  }
  try {
    return JSON.parse(result.stdout);
  } catch (err) {
    fail(`Ausgabe von "npm ls --omit=dev --all --json" ließ sich nicht lesen: ${err.message}`);
  }
}

// Issue #118/#122: Ein Paket, dessen `file:`-Auflösung auf ein Verzeichnis
// zeigt, ist kein Drittpaket, sondern ein lokales — ein Mitglied des
// npm-Workspace (läuft das Skript mit `--frontend` auf dessen Wurzel, listet
// `npm ls` die Mitglieder selbst als oberste Abhängigkeiten) oder ein per
// `file:` verlinktes Verzeichnis. Es wird nicht gelistet und nicht gegen die
// erlaubten Lizenzen geprüft (ein unveröffentlichtes Workspace-Mitglied hat
// oft gar kein `license`-Feld); seine eigenen Produktionsabhängigkeiten
// sammelt `walk` trotzdem weiter.
//
// Zeigt die `file:`-Auflösung dagegen auf einen Tarball (z. B.
// `"foo": "file:vendor/foo-1.0.0.tgz"`), ist das Paket echter Drittcode: npm
// entpackt ihn als gewöhnliches Verzeichnis nach `node_modules`. Es bleibt
// ein Drittpaket wie jede andere Auflösung (Registry, Git, Tarball-URL) und
// läuft durch die Lizenzprüfung.
//
// Tarball oder Verzeichnis entscheidet die Endung des aufgelösten Pfads,
// nicht ein Blick auf die Platte: Dieselbe Regel (`.tgz`, `.tar.gz`, `.tar`,
// Groß/Klein egal) nutzt npm selbst (`npm-package-arg`), um eine
// `file:`-Angabe als Tarball oder Verzeichnis einzuordnen. Sie stimmt damit
// genau mit dem überein, was npm installiert hat, und braucht den Tarball
// nicht — der muss nach der Installation nicht mehr vorhanden sein, und ein
// relativer `resolved`-Pfad hätte keine eindeutige Basis.
const TARBALL_PATH = /\.(?:tgz|tar\.gz|tar)$/i;

function isLocalDirectoryLink(info) {
  return (
    typeof info.resolved === "string" &&
    info.resolved.startsWith("file:") &&
    !TARBALL_PATH.test(info.resolved)
  );
}

// Hängt ein Mitglied von einem anderen ab, nennt `npm ls` es unter dem
// abhängigen Mitglied noch einmal, dort aber ohne `resolved` (gemessen mit
// npm 11). Deshalb zuerst alle lokalen Verzeichnis-Pakete im ganzen Baum
// sammeln; ein Eintrag OHNE `resolved` mit demselben Namen und derselben
// Version gilt dann ebenfalls als lokal. Ein Tarball-Paket trägt hier nichts
// bei (Issue #122), und ein Eintrag mit anderer Auflösung bleibt ein
// Drittpaket, auch bei gleichem Namen.
function collectLocalPackageKeys(npmLsTree) {
  const keys = new Set();
  function walk(node) {
    for (const [name, info] of Object.entries(node.dependencies ?? {})) {
      if (info.version && isLocalDirectoryLink(info)) keys.add(`${name}@${info.version}`);
      walk(info);
    }
  }
  walk(npmLsTree);
  return keys;
}

function localPackagePredicate(npmLsTree) {
  const localKeys = collectLocalPackageKeys(npmLsTree);
  return (name, info) =>
    isLocalDirectoryLink(info) ||
    (info.resolved === undefined && localKeys.has(`${name}@${info.version}`));
}

function collectProdPackages(npmLsTree) {
  const isLocalPackage = localPackagePredicate(npmLsTree);
  const seen = new Set();
  const out = [];
  function walk(node) {
    for (const [name, info] of Object.entries(node.dependencies ?? {})) {
      if (info.version && !isLocalPackage(name, info)) {
        const key = `${name}@${info.version}`;
        if (!seen.has(key)) {
          seen.add(key);
          out.push({ name, version: info.version });
        }
      }
      walk(info);
    }
  }
  walk(npmLsTree);
  return out;
}

function dirMatchesPackage(dir, name, version) {
  const pkgJsonPath = path.join(dir, "package.json");
  if (!fs.existsSync(pkgJsonPath)) return false;
  try {
    const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, "utf8"));
    return pkg.name === name && pkg.version === version;
  } catch {
    return false;
  }
}

// Sucht das Installationsverzeichnis eines npm-Pakets unter dem
// `node_modules` des per `--frontend` übergebenen Verzeichnisses — bei einer
// Workspace-Wurzel ist das genau der Ort, an den npm die Pakete aller
// Mitglieder hebt (Issue #118). Der häufige Fall (flaches
// `node_modules/<name>`) wird direkt getroffen; der rekursive Teil fängt den
// selteneren Fall ab, dass zwei Versionen desselben Pakets gleichzeitig
// installiert sind (verschachteltes `node_modules`, auch im Verzeichnis
// eines Workspace-Mitglieds, das npm als Verweis in `node_modules` ablegt).
function resolvePackageDir(frontendDir, name, version) {
  const direct = path.join(frontendDir, "node_modules", ...name.split("/"));
  if (dirMatchesPackage(direct, name, version)) return direct;
  const found = searchNodeModules(
    path.join(frontendDir, "node_modules"),
    name,
    version,
    0,
    new Set(),
  );
  if (found) return found;
  fail(`Installationsverzeichnis für npm-Paket ${name}@${version} nicht gefunden.`);
}

// Ein Eintrag in `node_modules` ist ein Verzeichnis oder ein Verweis auf
// eines (npm legt Workspace-Mitglieder und `file:`-Pakete als Symlink bzw.
// unter Windows als Junction ab).
function isDirectoryEntry(entry, full) {
  if (entry.isDirectory()) return true;
  if (!entry.isSymbolicLink()) return false;
  try {
    return fs.statSync(full).isDirectory();
  } catch {
    return false;
  }
}

function searchNodeModules(root, name, version, depth, visited) {
  if (depth > 6) return null;
  // Verweise können im Kreis führen (ein Workspace-Mitglied, dessen
  // `node_modules` wieder auf die Wurzel zeigt) — jedes echte Verzeichnis
  // wird nur einmal durchsucht.
  let realRoot;
  try {
    realRoot = fs.realpathSync(root);
  } catch {
    return null;
  }
  if (visited.has(realRoot)) return null;
  visited.add(realRoot);
  let entries;
  try {
    entries = fs.readdirSync(root, { withFileTypes: true });
  } catch {
    return null;
  }
  for (const entry of entries) {
    if (entry.name === ".bin") continue;
    const full = path.join(root, entry.name);
    if (!isDirectoryEntry(entry, full)) continue;
    if (entry.name.startsWith("@")) {
      const found = searchNodeModules(full, name, version, depth + 1, visited);
      if (found) return found;
      continue;
    }
    if (dirMatchesPackage(full, name, version)) return full;
    const nested = path.join(full, "node_modules");
    if (fs.existsSync(nested)) {
      const found = searchNodeModules(nested, name, version, depth + 1, visited);
      if (found) return found;
    }
  }
  return null;
}

function findFileByPrefix(dir, prefixPattern) {
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return null;
  }
  const re = new RegExp(`^${prefixPattern}`, "i");
  const match = entries.find((e) => e.isFile() && re.test(e.name));
  return match ? path.join(dir, match.name) : null;
}

function processNpmPackage(pkgDir, name, version, allowList) {
  const pkgJsonPath = path.join(pkgDir, "package.json");
  if (!fs.existsSync(pkgJsonPath)) {
    fail(`npm-Paket ${name}@${version}: ${pkgJsonPath} fehlt.`);
  }
  const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, "utf8"));
  const licenseExpr =
    typeof pkg.license === "string"
      ? pkg.license
      : typeof pkg.license === "object" && pkg.license?.type
        ? pkg.license.type
        : Array.isArray(pkg.licenses) && pkg.licenses.length > 0
          ? pkg.licenses.map((l) => l.type).filter(Boolean).join(" OR ")
          : null;
  if (!licenseExpr) {
    fail(`npm-Paket ${name}@${version} hat kein "license"-Feld in package.json.`);
  }
  if (!isLicenseAllowed(licenseExpr, allowList)) {
    fail(`npm-Paket ${name}@${version} hat eine nicht erlaubte Lizenz: "${licenseExpr}".`);
  }
  const licenseFile = findFileByPrefix(pkgDir, "licen[sc]e");
  if (!licenseFile) {
    fail(`npm-Paket ${name}@${version} hat keine Lizenzdatei in ${pkgDir}.`);
  }
  const noticeFile = findFileByPrefix(pkgDir, "notice");
  return {
    name,
    version,
    licenseExpr,
    licenseText: fs.readFileSync(licenseFile, "utf8"),
    notice: noticeFile ? fs.readFileSync(noticeFile, "utf8") : null,
  };
}

// npm-Seite (A1.1 b) für das per `--frontend` übergebene Verzeichnis —
// ein einzelnes Frontend-Paket oder die Wurzel eines npm-Workspace, der es
// als Mitglied enthält (Issue #118). Getrennt von `generate`, damit ein Test
// sie ohne `cargo-about` an einem Workspace-Fixture fahren kann.
function collectNpmProdPackages(frontendDir, allowList) {
  const npmTree = runNpmLs(frontendDir);
  return collectProdPackages(npmTree).map((p) =>
    processNpmPackage(resolvePackageDir(frontendDir, p.name, p.version), p.name, p.version, allowList),
  );
}

function processBundledDevDep(frontendDir, name, allowList) {
  const dir = path.join(frontendDir, "node_modules", ...name.split("/"));
  const pkgJsonPath = path.join(dir, "package.json");
  if (!fs.existsSync(pkgJsonPath)) {
    fail(
      `Build-Werkzeug "${name}" (A1.1 c) ist nicht unter ${dir} installiert — "npm ci" im Frontend fahren.`,
    );
  }
  const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, "utf8"));
  return processNpmPackage(dir, name, pkg.version, allowList);
}

// --- Rendern --------------------------------------------------------------

function groupByText(items, labelOf, textOf) {
  const map = new Map();
  for (const item of items) {
    const text = textOf(item);
    if (!map.has(text)) map.set(text, new Set());
    map.get(text).add(labelOf(item));
  }
  return [...map.entries()].map(([text, labels]) => ({ text, labels: [...labels].sort() }));
}

const SEPARATOR = "-".repeat(72);

function renderLicenseSection(title, groups) {
  const lines = [`## ${title}`, ""];
  if (groups.length === 0) {
    lines.push("(keine)", "");
  }
  for (const g of groups) {
    lines.push(`Verwendet von: ${g.labels.join(", ")}`, "", g.text.trimEnd(), "", SEPARATOR, "");
  }
  return lines.join("\n");
}

function renderNoticesSection(allNotices) {
  const lines = ["## Hinweisdateien (NOTICE)", ""];
  if (allNotices.length === 0) {
    lines.push("(keine)", "");
    return lines.join("\n");
  }
  for (const n of allNotices) {
    lines.push(`### ${n.label}`, "", n.content.trimEnd(), "", SEPARATOR, "");
  }
  return lines.join("\n");
}

function renderFontSection(fontLicensePath) {
  if (!fs.existsSync(fontLicensePath)) {
    fail(`Schriftlizenz-Datei nicht gefunden: ${fontLicensePath} (A6).`);
  }
  const content = fs.readFileSync(fontLicensePath, "utf8");
  return ["## Schriften", "", content.trimEnd(), ""].join("\n");
}

// --- main ------------------------------------------------------------------

function generate(args) {
  const allowList = parseAllowList(args.licenses);

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "smart-ssh-about-"));
  let aboutJson;
  try {
    const aboutTomlPath = writeGeneratedAboutToml(allowList, tmpDir);
    aboutJson = runCargoAbout(args.workspace, aboutTomlPath, tmpDir);
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }

  const rustGroups = aboutJson.licenses.map((lic) => ({
    text: lic.text,
    labels: lic.used_by.map((u) => `${u.crate.name} ${u.crate.version}`).sort(),
  }));
  const rustNotices = collectRustNotices(aboutJson);
  const vendoredLicenseNotices = collectVendoredLicenseNotices(aboutJson);

  const npmResults = collectNpmProdPackages(args.frontend, allowList);
  const npmGroups = groupByText(
    npmResults,
    (p) => `${p.name}@${p.version}`,
    (p) => p.licenseText,
  );
  const npmNotices = npmResults
    .filter((p) => p.notice)
    .map((p) => ({ label: `${p.name}@${p.version} (npm)`, content: p.notice }));

  const devResults = args.bundledDevDeps.map((name) =>
    processBundledDevDep(args.frontend, name, allowList),
  );
  const devGroups = groupByText(
    devResults,
    (p) => `${p.name}@${p.version}`,
    (p) => p.licenseText,
  );
  const devNotices = devResults
    .filter((p) => p.notice)
    .map((p) => ({ label: `${p.name}@${p.version} (devDependency, im Bundle)`, content: p.notice }));

  const sections = [
    MARKER,
    "",
    "Smart SSH enthaelt Software Dritter. Diese Datei listet die Lizenztexte",
    "der Rust- und npm-Abhaengigkeiten, der Build-Werkzeuge mit ausgeliefertem",
    "Code und der mitgelieferten Schriften (Spec 0099). Erzeugt bei jedem",
    "Release-Build; nicht Teil des Quelltexts.",
    "",
    renderLicenseSection("Rust-Abhaengigkeiten", rustGroups),
    renderLicenseSection("npm-Produktionsabhaengigkeiten", npmGroups),
    renderLicenseSection("Build-Werkzeuge mit ausgeliefertem Code (devDependencies)", devGroups),
    renderFontSection(args.fontLicense),
    renderNoticesSection([...rustNotices, ...vendoredLicenseNotices, ...npmNotices, ...devNotices]),
  ].join("\n");

  return { sections, stats: { rust: rustGroups.length, npm: npmResults.length, dev: devResults.length } };
}

function main() {
  const args = parseArgs(process.argv.slice(2));

  // A1.3: eine vorhandene Ausgabe wird zu Beginn gelöscht — ein
  // fehlgeschlagener Lauf darf nie die Datei eines früheren erfolgreichen
  // Laufs stehen lassen und so einen Erfolg vortäuschen.
  if (fs.existsSync(args.output)) fs.rmSync(args.output);

  try {
    const { sections, stats } = generate(args);
    fs.mkdirSync(path.dirname(args.output), { recursive: true });
    fs.writeFileSync(args.output, sections, "utf8");
    const bytes = Buffer.byteLength(sections, "utf8");
    console.log(
      `Drittlizenzen geschrieben nach ${args.output} (${bytes} Bytes; ${stats.rust} Rust-Lizenztext-Gruppen, ${stats.npm} npm-Pakete, ${stats.dev} Build-Werkzeuge).`,
    );
  } catch (err) {
    if (fs.existsSync(args.output)) fs.rmSync(args.output);
    const message = err instanceof GenerationError ? err.message : `${err.stack ?? err}`;
    console.error(`Drittlizenzen-Erzeugung fehlgeschlagen: ${message}`);
    process.exitCode = 1;
  }
}

// Nur ausführen, wenn direkt aufgerufen (nicht beim Import durch einen Test).
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}

export {
  parseArgs,
  parseAllowList,
  isLicenseAllowed,
  collectProdPackages,
  localPackagePredicate,
  collectNpmProdPackages,
  groupByText,
  generate,
};
