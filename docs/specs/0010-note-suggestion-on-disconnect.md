# Spec 0010 — Notiz-Vorschlag beim Beenden einer Sitzung

Status: umgesetzt
Zweck: Beim Trennen einer Sitzung fragt die App die KI einmal, ob etwas aus
der Sitzung in die Server-Notiz gehört, und zeigt einen Vorschlag zur
Bestätigung an.
Bezüge: Spec 0003 (Notizen, Bestätigungspflicht), Spec 0017 (Sitzungs-Tabs),
Spec 0019 (Änderungs-Vorschau), Spec 0023 (Ziel-Kennzeichnung), Spec 0034
(Sitzungstitel), Spec 0057 (Kontextaufbau, Kürzungs-Vorschlag), Spec 0096
(Schwärzung von Notiz-Vorschlägen), ADR 0017.

## 1. Überblick

Der Vorschlag ist ein optionales Extra beim Trennen. Er verzögert das
Trennen nicht, erscheint nur bei echtem Inhalt und wird nie ohne
Bestätigung übernommen.

## 2. Ablauf

1. Der Nutzer trennt die Sitzung. Die Verbindung wird **sofort** getrennt;
   die App wartet nicht auf die KI.
2. Danach, im Hintergrund, stellt die App der KI **eine** zusätzliche
   Frage auf Basis des bisherigen Sitzungsverlaufs, sinngemäß: „Gibt es aus
   dieser Sitzung Informationen, die für künftige Sitzungen an diesem
   Server als Notiz festgehalten werden sollten? Nur bei echtem Mehrwert,
   keine Wiederholung bestehender Notizinhalte." Diese Frage erscheint
   nirgends im Chat. Vorher läuft im selben Hintergrundschritt die
   Titelvergabe der Sitzung (Spec 0034); beide Anfragen laufen
   nacheinander, nicht parallel.
3. In dieser Anfrage kann die KI **nur** eine Notiz-Aktualisierung
   vorschlagen, keine Befehle, keine Dateizugriffe, keine Dokumente.
4. Schlägt die KI nichts vor, bricht die Anfrage ab, endet sie mit einem
   Fehler oder wird ihre Antwort abgeschnitten, passiert **nichts**: kein
   Hinweis, keine Fehlermeldung. Das ist der Regelfall, wenn in der Sitzung
   nichts Neues passiert ist. Dasselbe gilt, wenn die Schwärzung (Spec 0096)
   vom Vorschlag nichts übrig lässt.
5. Schlägt die KI eine Notiz-Aktualisierung vor, wird sie genauso
   bestätigt und gespeichert wie ein Vorschlag im laufenden Chat: Vorschau
   der Änderung (Spec 0019), Ziel deutlich benannt (Spec 0023), Übernehmen
   oder Ablehnen. Übernommen wird eine neue Revision mit der KI als
   Bearbeiter. Reagiert der Nutzer nicht innerhalb einer Stunde, gilt der
   Vorschlag als abgelehnt.
6. Der Vorschlag erscheint als **app-weite, nicht blockierende
   Benachrichtigung**, auch wenn der Nutzer inzwischen einen anderen
   Bildschirm oder Tab geöffnet hat. Sie ist zunächst kompakt (Ziel und
   „Anzeigen"); „Anzeigen" klappt die Vorschau auf.

## 3. Abgrenzung

- Der Vorschlag betrifft **nur die Notiz des Servers selbst**, nicht die
  Notiz seiner Gruppe. Gruppen-Notizen ändern sich nur von Hand oder über
  einen ausdrücklichen Vorschlag im laufenden Chat.
- Die Anfrage wird nur gestellt, wenn in der Sitzung **mindestens ein
  Befehl ausgeführt wurde und ein Ergebnis geliefert hat**, unabhängig von
  dessen Exit-Code. Eine Sitzung ganz ohne Befehlsausführung löst keine
  KI-Anfrage aus.
- Müsste die App den Kontext für diese Anfrage so stark kürzen, dass die
  KI die gespeicherte Notiz nur gekürzt sähe, entfällt der Vorschlag: ein
  Vorschlag auf dieser Grundlage könnte Notizinhalt verlieren.
- Der Vorschlagstext wird vor der Anzeige geschwärzt (Spec 0096); angezeigt
  und gespeichert wird dieselbe geschwärzte Fassung.
- Hat dieser Schritt einen Vorschlag angezeigt, erscheint am selben
  Verbindungsende kein Kürzungs-Vorschlag für eine große Notiz (Spec 0057,
  Spec 0079). Es gibt nie zwei konkurrierende Notiz-Dialoge zu einem
  Verbindungsende.

## 4. Grenzen

- Es gibt keine Einstellung, die diese Nachfrage abschaltet.
