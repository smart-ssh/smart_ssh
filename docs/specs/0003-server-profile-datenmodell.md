# Spec 0003 — Server-Profile, Gruppen, Credentials und Kontextnotizen

Status: umgesetzt
Zweck: Beschreibt, was ein Server-Profil und eine Gruppe enthalten, wie Credentials referenziert werden und wie Kontextnotizen für die KI gepflegt und vererbt werden.
Bezüge: Spec 0002 (Filter-Engine nutzt Tags der Server), Spec 0004 (Speicherung), Spec 0101 (Ablage der Secrets), Spec 0010 und 0023 (Notizvorschläge), ADR 0004 (keine Kürzung des Notizkontexts), ADR 0050 (Hinweis bei langen Notizen).

## 1. Ziel

Ein Nutzer verwaltet Server-Verbindungsprofile, ordnet sie in Gruppen, hinterlegt
Zugangsdaten sicher und pflegt Freitext-Kontextnotizen pro Server und pro Gruppe,
die der KI als Zusatzkontext dienen. Notizen können vom Nutzer und — nach
Bestätigung — von der KI geändert werden.

## 2. Gruppen

- Gruppen bilden einen Baum: eine Gruppe hat höchstens eine übergeordnete Gruppe,
  die Tiefe ist nicht begrenzt.
- Eine Gruppe hat einen Namen, eine Notiz (aktueller KI-Kontext, s. Abschnitt 5)
  sowie Erstell- und Änderungszeitpunkt.
- **Gruppen sind von den Tags der Filter-Engine getrennt.** Gruppen dienen der
  Organisation und dem Notizkontext; Tags (z. B. `production`) steuern die
  Policy (Spec 0002). Ein Server kann in einer Gruppe liegen und unabhängig davon
  Tags tragen. Die Gruppenzugehörigkeit verändert nie die Tags oder die Policy
  eines Servers.
- Eine Gruppe lässt sich nicht unter sich selbst oder unter einen ihrer
  Nachfolger hängen (kein Zyklus).

## 3. Server-Profil

Ein Server hat:

- Name, Host, Port, Benutzername
- optional eine Gruppe
- Tags (Policy-Scopes, Spec 0002)
- eine Anmeldeart (s. unten)
- eine Notiz (aktueller KI-Kontext, s. Abschnitt 5)
- optional einen Jump-Host (anderer gespeicherter Server, Spec 0005)
- eine Stufe für die Eskalation nach eingelesenem Serverinhalt und einen Schalter
  für die KI-Prüfung auf eingeschleuste Anweisungen (Spec 0039)
- optional einen Pfad für den erhöhten Dateibrowser-Modus (Spec 0067)
- optional ein Startverzeichnis (Spec 0102)
- Erstell- und Änderungszeitpunkt

Anmeldearten:

- **Passwort**
- **Privater Schlüssel** mit optionaler Passphrase
- **Schlüsseldatei** auf der Platte mit optionaler Passphrase (Spec 0076); der
  Pfad wird so gespeichert, wie der Nutzer ihn eingegeben hat
- **Zertifikat** (Zertifikat und Schlüssel)
- **SSH-Agent**

## 4. Credentials

- Ein Profil trägt nur **Verweise** auf Credentials, nie Passwörter, private
  Schlüssel, Passphrasen oder Zertifikatsinhalte selbst.
- Die Secrets selbst liegen in der verschlüsselten Datenbank (Spec 0101, Spec
  0096).
- Secrets werden in der Anwendung nie geloggt oder in Debug-Ausgaben
  ausgegeben.
- Der Pfad einer Schlüsseldatei ist kein Secret und darf in Datenbank, Log und
  Oberfläche stehen.

## 5. Kontextnotizen

Eine Notiz ist ein Freitextfeld pro Server und pro Gruppe, das der KI als Kontext
mitgegeben wird (z. B. „PHP 8.2 und MySQL 8 liegen unter `/opt/lamp`"). Sie ist
vom Nutzer editierbar und kann von der KI zur Änderung vorgeschlagen werden.

### 5.1 Effektiver Kontext

- Der KI-Kontext einer Sitzung setzt sich aus den Notizen der Gruppenkette von
  der Wurzel bis zur unmittelbaren Gruppe und danach der Notiz des Servers
  zusammen, vom Allgemeinen zum Spezifischen.
- Jeder Abschnitt trägt eine Überschrift mit seiner Quelle (`## Kontext: <Gruppe>`,
  `## Kontext: Server "<Name>"`).
- Leere oder nur aus Leerraum bestehende Notizen erzeugen keinen Abschnitt.
- Der zusammengesetzte Text wird **nicht gekürzt oder priorisiert** (ADR 0004).
- Ist die Gruppenkette zyklisch, ergibt das einen Fehler statt einer Endlosschleife.
- Die Notiztexte gelten als nicht vertrauenswürdiger Inhalt und werden vor der
  Übergabe an die KI markiert und redigiert (Spec 0039, Spec 0016).
- Der Nutzer kann den effektiven Kontext eines Servers vorab ansehen.

### 5.2 Änderungen durch die KI

- Eine Notizänderung durch die KI ist **kein Shell-Kommando** und läuft nicht
  durch die Filter-Engine.
- Die KI schlägt den vollständigen neuen Text vor; der Nutzer sieht einen Diff
  (alt/neu) und bestätigt oder verwirft.
- Ein Vorschlag wird nie automatisch übernommen, unabhängig von Policy-Regeln.
  Das ist bewusst strenger als bei automatisch ausführbaren Kommandos, weil eine
  Notiz den Kontext künftiger Sitzungen dauerhaft beeinflusst.

### 5.3 Änderungshistorie

- Jede Änderung einer Notiz (durch den Nutzer oder die KI nach Bestätigung)
  erzeugt eine Revision mit Inhalt, Zeitpunkt und Urheber (Nutzer, oder KI mit
  Anbieter und Modell). Der lokale Pseudo-Server (Spec 0032) hat keine Historie.
- Das Notizfeld selbst hält nur den aktuellen Stand; die Historie ist davon
  getrennt, nur lesbar und chronologisch einsehbar.
- Der Nutzer kann eine Notiz auf eine frühere Revision zurücksetzen; das ist
  selbst wieder eine neue Revision.

## 6. Grenzen

- Server erben die Tags ihrer Gruppe nicht; Gruppe und Policy bleiben getrennt
  (Abschnitt 2).
- Notizen haben keine Längen- oder Token-Begrenzung (ADR 0004); der Nutzer wird
  bei sehr langen Notizen gefragt, ob sie verkürzt werden sollen (ADR 0050).
- Vorschläge der KI zu Notizen werden immer manuell bestätigt; es gibt keine
  Automatik.
