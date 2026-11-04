import { describe, expect, it } from "vitest";
import { displayPath, joinPath, localBaseName, parentPath } from "./remotePath";

// Spec 0020, Abschnitt 5 — reine Pfad-Logik für den manuellen Dateibrowser.

describe("parentPath", () => {
  it("bleibt im Startverzeichnis, wenn es dort schon ist", () => {
    expect(parentPath(".")).toBe(".");
  });

  it("geht ein relatives Segment nach oben", () => {
    expect(parentPath("./sub")).toBe(".");
    expect(parentPath("./sub/tiefer")).toBe("./sub");
  });

  it("geht ein absolutes Segment nach oben, bleibt am Dateisystem-Root stehen", () => {
    expect(parentPath("/etc/nginx")).toBe("/etc");
    expect(parentPath("/etc")).toBe("/");
  });
});

describe("joinPath", () => {
  it("lässt den Punkt-Präfix weg, wenn vom Startverzeichnis aus verbunden wird", () => {
    expect(joinPath(".", "datei.txt")).toBe("datei.txt");
  });

  it("verbindet ohne doppelten Schrägstrich", () => {
    expect(joinPath("/etc/", "hosts")).toBe("/etc/hosts");
    expect(joinPath("/etc", "hosts")).toBe("/etc/hosts");
  });
});

describe("displayPath", () => {
  it("zeigt das Startverzeichnis als Tilde", () => {
    expect(displayPath(".")).toBe("~");
  });

  it("lässt andere Pfade unverändert", () => {
    expect(displayPath("/etc/nginx")).toBe("/etc/nginx");
  });
});

describe("localBaseName", () => {
  it("extrahiert das letzte Segment aus einem POSIX-Pfad", () => {
    expect(localBaseName("/Users/stefan/Downloads/bericht.pdf")).toBe("bericht.pdf");
  });

  it("extrahiert das letzte Segment aus einem Windows-Pfad", () => {
    expect(localBaseName("C:\\Users\\stefan\\Downloads\\bericht.pdf")).toBe("bericht.pdf");
  });
});
