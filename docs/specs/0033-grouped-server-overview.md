# Spec: Gruppierte Server-Übersicht

Status: Entwurf
Modul: `frontend/` (Haupt-Serverübersicht)
Abhängigkeiten: Gruppen-Datenmodell (Spec 0003), bestehende Gruppen-Baum-
Logik der Verwaltungs-Sidebar (Spec 0008, Abschnitt 6), lokaler
Pseudo-Server (Spec 0032)

## 1. Problem

Die Haupt-Serverübersicht (der Bildschirm, über den ein Server zum
Verbinden ausgewählt wird — zu unterscheiden von der Sidebar im
Bearbeiten-Formular aus Spec 0008) zeigt Server aktuell als flache Liste,
ohne die längst vorhandene Gruppenstruktur (Spec 0003) sichtbar zu machen.
Bei mehr als einer Handvoll Server wird das schnell unübersichtlich.

## 2. Ziel

Dieselbe Gruppierung, die im Bearbeiten-Formular bereits als Baum sichtbar
ist (Spec 0008, Abschnitt 6), auch in der Haupt-Übersicht — Server werden
nach ihrer Gruppenzugehörigkeit abschnittsweise dargestellt, nicht mehr als
ein großer Haufen.

## 3. Struktur

- Ein einklappbarer Abschnitt pro Gruppe (inkl. verschachtelter
  Untergruppen, eingerückt entsprechend der Hierarchietiefe).
- Ein zusätzlicher Abschnitt **"Ohne Gruppe"** für Server ohne
  `group_id`, am Ende der Liste.
- Der lokale Pseudo-Server (Spec 0032) erscheint **fest angepinnt oberhalb**
  aller Gruppen-Abschnitte, niemals innerhalb eines Ordners — visuell klar
  als eigene Kategorie abgesetzt (z. B. durch eine Trennlinie).

## 4. Verhalten

- Klick auf eine Gruppen-Kopfzeile klappt sie ein-/aus.
- Der Auf-/Zugeklappt-Zustand bleibt mindestens für die laufende
  App-Sitzung erhalten (State im Frontend); ob er darüber hinaus dauerhaft
  gespeichert wird (`tauri-plugin-store`, Spec 0024-Muster), ist ein
  offener Punkt (Abschnitt 6) — kein Blocker für diese Spec.
- Leere Gruppen (keine direkten Server, aber ggf. Untergruppen mit
  Servern) werden trotzdem angezeigt, damit die Hierarchie nachvollziehbar
  bleibt, nicht übersprungen.

## 5. Wiederverwendung statt Neubau

Die Logik "aus `list_groups()` + `list_servers()` einen Baum aufbauen"
existiert bereits für die Verwaltungs-Sidebar (Spec 0008). Extrahiere diese
Logik in eine gemeinsame Komponente/einen gemeinsamen Hook, den sowohl die
Verwaltungs-Sidebar als auch die neue gruppierte Haupt-Übersicht nutzen —
keine zweite Implementierung derselben Baum-Aufbau-Logik. Die beiden
Ansichten dürfen sich in Darstellung/Interaktion unterscheiden (die eine
öffnet ein Bearbeiten-Formular, die andere verbindet direkt), aber nicht in
der zugrunde liegenden Datenstruktur-Aufbereitung.

## 6. Offene Punkte

- Dauerhafte Speicherung des Ein-/Ausklapp-Zustands über einen Neustart
  hinweg — naheliegend, aber nicht zwingend Teil dieses Schritts.
- Soll es eine Volltextsuche/Filter über alle Server hinweg geben (die
  Gruppenstruktur bei Bedarf temporär "aufhebt")? Nicht Teil dieser Spec,
  denkbare spätere Ergänzung bei wachsender Serverzahl.
