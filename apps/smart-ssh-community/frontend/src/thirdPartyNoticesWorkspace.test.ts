// @vitest-environment node
//
// Issue #118 (Spec 0099, A1.1/A1.4): Das Drittlizenz-Skript läuft auch, wenn
// `--frontend` auf die Wurzel eines npm-Workspace zeigt, der das Frontend als
// Mitglied enthält. Die Mitglieder selbst (`file:`-Auflösung in `npm ls`,
// ohne `license`-Feld) sind keine Drittpakete; ihre Produktionsabhängigkeiten
// erscheinen genau einmal, auch die nicht an die Wurzel gehobenen. Ein echtes
// Drittpaket ohne erlaubte Lizenz bricht weiterhin ab.
//
// Das Fixture wird zur Laufzeit in einem temporären Verzeichnis gebaut (kein
// `npm install`, kein Netz): `node_modules` von Hand, die Mitglieder als
// Verweis darin — wie npm sie ablegt. Gefahren wird das echte `npm ls`.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  collectNpmProdPackages,
  collectProdPackages,
  localPackagePredicate,
} from "../../../../scripts/generate-third-party-notices.mjs";

const ALLOW_LIST = ["MIT", "Apache-2.0"];

let fixtureRoot: string | null = null;

afterEach(() => {
  if (fixtureRoot) fs.rmSync(fixtureRoot, { recursive: true, force: true });
  fixtureRoot = null;
});

function writeJson(file: string, value: unknown) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, JSON.stringify(value, null, 2));
}

function writeThirdParty(dir: string, name: string, version: string, license: string | null) {
  writeJson(path.join(dir, "package.json"), {
    name,
    version,
    ...(license === null ? {} : { license }),
  });
  fs.writeFileSync(path.join(dir, "LICENSE"), `${license ?? "?"} license text of ${name}@${version}\n`);
}

// Workspace-Wurzel mit zwei Mitgliedern ohne `license`-Feld:
// - `app` (Produktion: `dep-a`, das Mitglied `lib`; Entwicklung: `dev-only`)
// - `lib` (Produktion: `dep-a`, `dep-c@2` — nicht gehoben, liegt im
//   `node_modules` von `lib`, weil an der Wurzel `dep-c@1` liegt)
function buildWorkspace(depALicense: string | null = "MIT"): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "smart-ssh-notices-ws-"));
  fixtureRoot = root;
  writeJson(path.join(root, "package.json"), {
    name: "fixture-root",
    private: true,
    workspaces: ["packages/*"],
  });
  writeJson(path.join(root, "packages/app/package.json"), {
    name: "app",
    version: "0.0.0",
    private: true,
    dependencies: { "dep-a": "^1.0.0", lib: "0.0.0" },
    devDependencies: { "dev-only": "^1.0.0" },
  });
  writeJson(path.join(root, "packages/lib/package.json"), {
    name: "lib",
    version: "0.0.0",
    private: true,
    dependencies: { "dep-a": "^1.0.0", "dep-c": "^2.0.0" },
  });
  writeJson(path.join(root, "packages/other/package.json"), {
    name: "other",
    version: "0.0.0",
    private: true,
    dependencies: { "dep-c": "^1.0.0" },
  });
  const nm = path.join(root, "node_modules");
  writeThirdParty(path.join(nm, "dep-a"), "dep-a", "1.0.0", depALicense);
  writeThirdParty(path.join(nm, "dep-c"), "dep-c", "1.0.0", "MIT");
  writeThirdParty(path.join(nm, "dev-only"), "dev-only", "1.0.0", "GPL-3.0-only");
  writeThirdParty(
    path.join(root, "packages/lib/node_modules/dep-c"),
    "dep-c",
    "2.0.0",
    "Apache-2.0",
  );
  // npm legt Workspace-Mitglieder als Verweis in `node_modules` ab; unter
  // Windows als Junction (braucht keine besonderen Rechte, absoluter Pfad).
  for (const member of ["app", "lib", "other"]) {
    fs.symlinkSync(path.join(root, "packages", member), path.join(nm, member), "junction");
  }
  return root;
}

describe("Drittlizenzen: Lauf an einer npm-Workspace-Wurzel (Issue #118)", () => {
  it("listet die Drittabhängigkeiten der Mitglieder genau einmal, die Mitglieder selbst nicht", () => {
    const root = buildWorkspace();
    const result = collectNpmProdPackages(root, ALLOW_LIST);
    const labels = result.map((p) => `${p.name}@${p.version}`).sort();

    expect(labels).toEqual(["dep-a@1.0.0", "dep-c@1.0.0", "dep-c@2.0.0"]);
    // Das nicht gehobene `dep-c@2` kommt aus dem `node_modules` von `lib`.
    expect(result.find((p) => p.version === "2.0.0")?.licenseText).toContain(
      "Apache-2.0 license text of dep-c@2.0.0",
    );
  });

  it("bricht weiterhin ab, wenn ein Drittpaket eine nicht erlaubte Lizenz hat", () => {
    const root = buildWorkspace("GPL-3.0-only");
    expect(() => collectNpmProdPackages(root, ALLOW_LIST)).toThrow(
      /dep-a@1\.0\.0 hat eine nicht erlaubte Lizenz/,
    );
  });

  it("bricht weiterhin ab, wenn ein Drittpaket gar kein Lizenzfeld hat", () => {
    const root = buildWorkspace(null);
    expect(() => collectNpmProdPackages(root, ALLOW_LIST)).toThrow(
      /dep-a@1\.0\.0 hat kein "license"-Feld/,
    );
  });
});

describe("Drittlizenzen: lokale Pakete in der npm-ls-Ausgabe (Issue #118)", () => {
  it("überspringt nur `file:`-Pakete und sammelt deren Abhängigkeiten weiter", () => {
    const packages = collectProdPackages({
      dependencies: {
        member: {
          version: "0.0.0",
          resolved: "file:../packages/member",
          dependencies: {
            "from-registry": { version: "1.0.0", resolved: "https://registry.npmjs.org/x.tgz" },
            linked: {
              version: "0.1.0",
              resolved: "file:../vendor/linked",
              dependencies: { deep: { version: "3.0.0" } },
            },
          },
        },
        // So nennt `npm ls` ein Mitglied, von dem ein anderes abhängt: an
        // zweiter Stelle ohne `resolved`.
        other: { version: "0.0.0", dependencies: { member: { version: "0.0.0" } } },
        "from-git": { version: "2.0.0", resolved: "git+https://example.invalid/x.git#abc" },
        "from-registry": { version: "1.0.0" },
        // Gleicher Name wie ein Mitglied, aber eigene Auflösung: Drittpaket.
        linked: { version: "0.1.0", resolved: "https://registry.npmjs.org/linked.tgz" },
      },
    });
    expect(packages.map((p) => `${p.name}@${p.version}`).sort()).toEqual([
      "deep@3.0.0",
      "from-git@2.0.0",
      "from-registry@1.0.0",
      "linked@0.1.0",
      "other@0.0.0",
    ]);
  });
});

// Issue #122 (Spec 0099, A1.1/A1.3, T5a): Ein aus einem lokalen Tarball
// installiertes Paket (`"foo": "file:vendor/foo-1.0.0.tgz"`) hat in `npm ls`
// ebenfalls eine `file:`-Auflösung, ist aber echter Drittcode: npm entpackt
// es als gewöhnliches Verzeichnis nach `node_modules`. Es wird gelistet und
// gegen die erlaubten Lizenzen geprüft wie ein Registry-Paket.
//
// Fixture ohne `npm install` und ohne den Tarball selbst: `npm ls` liest die
// Auflösung aus der `package-lock.json`, so wie npm sie bei der Installation
// schreibt.
function buildTarballProject(fooLicense: string | null = "MIT"): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "smart-ssh-notices-tgz-"));
  fixtureRoot = root;
  const manifest = {
    name: "fixture-tarball",
    version: "0.0.0",
    private: true,
    dependencies: { foo: "file:vendor/foo-1.0.0.tgz", linked: "file:vendor/linked" },
  };
  writeJson(path.join(root, "package.json"), manifest);
  writeJson(path.join(root, "vendor/linked/package.json"), { name: "linked", version: "0.1.0" });
  writeJson(path.join(root, "package-lock.json"), {
    name: manifest.name,
    version: manifest.version,
    lockfileVersion: 3,
    requires: true,
    packages: {
      "": {
        name: manifest.name,
        version: manifest.version,
        dependencies: manifest.dependencies,
      },
      "node_modules/foo": { version: "1.0.0", resolved: "file:vendor/foo-1.0.0.tgz" },
      "node_modules/linked": { resolved: "vendor/linked", link: true },
      "vendor/linked": { name: "linked", version: "0.1.0" },
    },
  });
  // Entpackter Tarball: ein echtes Verzeichnis, kein Verweis.
  writeThirdParty(path.join(root, "node_modules/foo"), "foo", "1.0.0", fooLicense);
  // Verlinktes Verzeichnis ohne Lizenzfeld: bleibt lokal.
  fs.symlinkSync(
    path.join(root, "vendor/linked"),
    path.join(root, "node_modules/linked"),
    "junction",
  );
  return root;
}

describe("Drittlizenzen: Pakete aus einem lokalen Tarball (Issue #122)", () => {
  it("listet das Tarball-Paket mit Lizenztext, das verlinkte Verzeichnis nicht", () => {
    const root = buildTarballProject();
    const result = collectNpmProdPackages(root, ALLOW_LIST);

    expect(result.map((p) => `${p.name}@${p.version}`)).toEqual(["foo@1.0.0"]);
    expect(result[0].licenseText).toContain("MIT license text of foo@1.0.0");
  });

  it("bricht ab, wenn das Tarball-Paket eine nicht erlaubte Lizenz hat", () => {
    const root = buildTarballProject("GPL-3.0-only");
    expect(() => collectNpmProdPackages(root, ALLOW_LIST)).toThrow(
      /foo@1\.0\.0 hat eine nicht erlaubte Lizenz/,
    );
  });

  it("bricht ab, wenn das Tarball-Paket gar kein Lizenzfeld hat", () => {
    const root = buildTarballProject(null);
    expect(() => collectNpmProdPackages(root, ALLOW_LIST)).toThrow(
      /foo@1\.0\.0 hat kein "license"-Feld/,
    );
  });

  it("zählt Tarball-Auflösungen als Drittpakete, auch für gleichnamige Einträge ohne `resolved`", () => {
    const packages = collectProdPackages({
      dependencies: {
        "tgz-abs": { version: "1.0.0", resolved: "file:/abs/vendor/tgz-abs-1.0.0.tgz" },
        "tgz-gz": { version: "1.0.0", resolved: "file:vendor/tgz-gz-1.0.0.TAR.GZ" },
        "tgz-tar": { version: "1.0.0", resolved: "file:../vendor/tgz-tar-1.0.0.tar" },
        "dir-link": { version: "0.1.0", resolved: "file:../vendor/dir-link" },
        // Ein Verzeichnis, dessen Name nur nach Tarball klingt, bleibt lokal.
        "dir-dotted": { version: "0.2.0", resolved: "file:../vendor/dir.tgz-src" },
        consumer: {
          version: "2.0.0",
          dependencies: {
            // Gleicher Name und gleiche Version wie das Tarball-Paket, ohne
            // `resolved`: kein lokales Paket.
            "tgz-abs": { version: "1.0.0" },
            "dir-link": { version: "0.1.0" },
          },
        },
      },
    });
    expect(packages.map((p) => `${p.name}@${p.version}`).sort()).toEqual([
      "consumer@2.0.0",
      "tgz-abs@1.0.0",
      "tgz-gz@1.0.0",
      "tgz-tar@1.0.0",
    ]);
  });

  it("macht einen gleichnamigen Eintrag ohne `resolved` nicht lokal, nur ein verlinktes Verzeichnis", () => {
    const tree = {
      dependencies: {
        "tgz-pkg": { version: "1.0.0", resolved: "file:/abs/vendor/tgz-pkg-1.0.0.tgz" },
        "dir-link": { version: "0.1.0", resolved: "file:../vendor/dir-link" },
      },
    };
    const isLocal = localPackagePredicate(tree);
    expect(isLocal("tgz-pkg", { version: "1.0.0" })).toBe(false);
    expect(isLocal("dir-link", { version: "0.1.0" })).toBe(true);
  });
});
