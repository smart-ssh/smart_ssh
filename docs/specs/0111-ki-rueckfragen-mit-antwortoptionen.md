# Spec 0111 — KI-Rückfragen mit Antwortoptionen

Status: umgesetzt (Werkzeug, Ereignis, Speicherung, Verlauf, Fragekarte im
Chat).
Zweck: Braucht die KI eine Entscheidung des Nutzers zwischen konkreten
Alternativen, kann sie genau eine Frage mit 2 bis 4 Antwortoptionen stellen,
statt im Fließtext zu raten oder nachzufragen.
Bezüge: Spec 0006 (Redaction), Spec 0012 (KI-Dokumente), Spec 0034 (Speicherung
von Chats), Spec 0080 (leere KI-Antwort).

## 1. Die Frage

- Die KI stellt die Frage mit dem Werkzeug `ask_user` (in jeder Edition und
  Sprache verfügbar): **eine** Frage je Aufruf, 2 bis 4 Optionen, je Option ein
  kurzes Label und optional ein kurzer beschreibender Satz. Standard ist die
  Einfachauswahl; die KI kann Mehrfachauswahl verlangen.
- Der Nutzer kann stattdessen immer in eigenen Worten antworten. Jede Antwort
  kommt als normale Nutzer-Nachricht; die App erzeugt nie selbst eine
  Nutzer-Nachricht.
- Die Systemanweisung sagt der KI: nur bei einer Entscheidung zwischen
  konkreten Alternativen, nicht für offene Fragen, immer nur eine Frage, danach
  anhalten und warten. Eine angeklickte Antwort ersetzt nie die Bestätigung
  eines Kommandos; Kommandos laufen weiter über `suggest_command`.

## 2. Verhalten der App

- Eine gültige Frage führt nichts aus: keine Filter-Prüfung, keine
  Bestätigung, keine Folgerunde. Die Antwort der KI endet damit; die Runde
  gilt nicht als leer (Spec 0080).
- Die App meldet die Frage dem Frontend mit dem Ereignis `chat-question-asked`
  (Sitzung, Frage-ID, Frage, Optionen mit Label und Beschreibung,
  Mehrfachauswahl). Die Optionen kommen als strukturierte Daten, nie aus
  Text herausgelesen.
- Gültig ist eine Frage mit nicht leerem Text bis 500 Zeichen und 2 bis 4
  Optionen, je mit nicht leerem Label bis 80 Zeichen und einer Beschreibung bis
  200 Zeichen. Jede andere Frage erzeugt keine Fragekarte und kein Ereignis;
  stattdessen erscheint die Frage mit den Labels als Aufzählung als normale
  Assistenten-Nachricht (angezeigt und gespeichert), ohne Fehlermeldung.
- Wird die Antwort gestoppt oder ist sie abgeschnitten, erscheint keine
  Frage und nichts wird gespeichert.
- Fragetext, Labels und Beschreibungen laufen durch dieselbe Redaction wie
  anderer KI-Text, bevor sie angezeigt, protokolliert oder gespeichert werden,
  und vor jedem erneuten Senden.

## 3. Verlauf

- Die Frage wird mit ihrer Frage-ID im Chatverlauf gespeichert und beim
  Wiederöffnen der Sitzung als Frage-Eintrag (mit derselben ID) geliefert.
- Beim Senden des Verlaufs an die KI steht die Frage als Assistenten-Text mit
  der Frage und den nummerierten Labels, damit die KI die nächste
  Nutzer-Nachricht als Antwort versteht.
- Eine wieder geladene Frage erscheint als Fragekarte (Abschnitt 5), mit dem
  Zustand, der sich aus dem Verlauf ergibt.

## 5. Fragekarte im Chat

- Die Karte zeigt die Frage und 2 bis 4 Optionen, je mit Label und, falls
  vorhanden, der Beschreibung darunter. Alle Texte erscheinen als reiner Text,
  nie als HTML oder Markdown.
- Einfachauswahl: Ein Klick auf eine Option sendet deren Label sofort als
  normale Nutzer-Nachricht. Mehrfachauswahl: Die Optionen sind Kästchen; der
  Knopf „Senden“ (deaktiviert, solange nichts angehakt ist) sendet die
  angehakten Labels in einer Nachricht, ein Label je Zeile, in der
  Reihenfolge der Optionen.
- Tastatur: Hat die Karte (oder ein Element darin) den Fokus, wählen die
  Tasten 1 bis 4 die entsprechende Option (Einfachauswahl: sendet;
  Mehrfachauswahl: schaltet um). Alle Optionen sind mit Tab erreichbar und
  zeigen einen sichtbaren Fokus. Im Eingabefeld haben die Tasten ihre normale
  Wirkung. Die Karte ist eine Gruppe, deren Name die Frage ist.
- Der Nutzer kann die Karte ignorieren und wie gewohnt tippen. Läuft gerade
  eine Antwort, wird ein Klick wie eine getippte Nachricht eingereiht
  (Spec 0066).
- Sperre: Sobald nach der Frage eine Nutzer-Nachricht gesendet wurde (Option,
  Mehrfachauswahl oder getippt), ist die Karte gesperrt: Optionen
  deaktiviert, gewählte Optionen mit Häkchen und Hervorhebung markiert, oder
  der Hinweis „In eigenen Worten beantwortet“, wenn die Nachricht zu keiner
  Option passt. Nur die neueste unbeantwortete Karte ist bedienbar; ältere
  sind gesperrt.
- Nach dem Wiederöffnen eines Chats gilt dieselbe Regel, abgeleitet aus dem
  Verlauf: Die erste Nutzer-Nachricht nach der Frage entscheidet. Passt sie
  genau zu einem Label (bei Mehrfachauswahl: jede Zeile genau zu einem Label),
  sind diese Optionen markiert, sonst gilt „In eigenen Worten beantwortet“.
  Eine Frage ohne spätere Nutzer-Nachricht ist bedienbar.
- Ein Klick ist nie mehr als eine Nutzer-Nachricht: Er beantwortet oder
  bestätigt kein Kommando und führt nichts aus.

## 4. Grenzen

- `ask_user` ist nie ausführbar und für externe Clients (MCP) nicht erreichbar.
