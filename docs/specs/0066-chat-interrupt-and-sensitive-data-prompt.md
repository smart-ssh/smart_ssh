# Spec 0066 — Chat jederzeit unterbrechbar, Regel für sensible Daten

Status: umgesetzt
Zweck: Der Nutzer kann eine laufende KI-Antwort jederzeit stoppen und jederzeit
eine Nachricht senden; die KI wird angewiesen, Geheimnisse nicht in den Chat
zu lesen.
Bezüge: Spec 0021 (Fortsetzung, „Automatik stoppen"), Spec 0065 (abgeschnittene
Vorschläge werden nie ausgeführt), Spec 0002 (Filter-Engine, Bestätigung),
Spec 0020 (Remote-Datei lesen), Spec 0036 (Verschlüsselung), ADR 0057.

## Entscheidungen

1. **Senden während die KI arbeitet reiht ein.** Die Nachricht unterbricht
   nichts, sondern geht als eigener Text-Block mit der **nächsten Anfrage** an
   die KI (typisch zusammen mit dem Kommando-Ergebnis der nächsten
   Fortsetzungsrunde). Endet der Turn ohne weitere Runde, geht sie danach als
   normale Nachricht raus.
2. **Stopp lässt ein laufendes Remote-Kommando weiterlaufen** — nur die KI
   stoppt. Das Kommando bleibt über den Kommando-Abbruch-Knopf abbrechbar.

## 1. Stopp bricht den laufenden KI-Request ab

- „Automatik stoppen" (bzw. ein „Stopp"-Knopf während einer Antwort) bricht
  den **gerade laufenden** KI-Stream sofort ab, nicht erst an der nächsten
  Rundengrenze. Die Verbindung zum Anbieter wird geschlossen; es werden keine
  weiteren Tokens erzeugt oder bezahlt. Der Abbruch greift auch, wenn die App
  zwischen zwei Versuchen eines Retries wartet: nach dem Stopp geht keine
  zweite Anfrage raus.
- **Invariante:** Aus einem abgebrochenen Stream wird **nie** ein
  Kommandovorschlag an Bestätigung oder Ausführung weitergegeben — auch nicht,
  wenn der Abbruch nach einem vollständigen Vorschlags-Block liegt. Der
  Anbieter hält Vorschläge bis zum Abbruchgrund zurück (Spec 0065), ein
  Abbruch davor verwirft sie.
- Bereits gestreamter Text bleibt sichtbar, mit dezenter Markierung
  „Abgebrochen". Die Markierung ist ein Oberflächen-Flag und kein Text im
  Inhalt, damit Ausgabe sie nicht fälschen kann.
- Ein zum Zeitpunkt des Stopps **offener Bestätigungsdialog bleibt stehen**;
  der Nutzer entscheidet selbst (Spec 0021, Abschnitt 5). Nach der
  Entscheidung folgt keine weitere Runde.
- Ein schon abgeholter, aber noch nicht gestarteter automatisch auszuführender
  Vorschlag wird nach einem Stopp nicht mehr ausgeführt und als abgebrochen
  gemeldet (nur im eigenen Chat, nicht bei MCP).
- Ein laufendes Remote-Kommando läuft zu Ende (Entscheidung 2); sein
  Ergebnis wird angezeigt, aber **nicht** mehr automatisch an die KI
  zurückgegeben (keine neue Runde nach Stopp).
- Liegen beim Stopp eingereihte Nachrichten vor (Abschnitt 2), werden sie
  danach als normale neue Nachricht gesendet.

## 2. Nachricht jederzeit senden

- Eingabefeld und Senden-Knopf sind **nie** wegen einer laufenden Antwort
  gesperrt.
- Senden während eines Turns reiht ein (Entscheidung 1): Die Nachricht
  erscheint sofort im Chat (markiert „wird mit der nächsten Anfrage
  gesendet"); der laufende Request wird nicht unterbrochen.
- An der nächsten Rundengrenze wird sie als **eigener Text-Block** in den
  nächsten Nutzer-Turn gelegt — **außerhalb** jeder Umzäunung von
  Server-Ausgabe (`<stdout>`/`<stderr>`/`<remote_file>`), klar als
  Nutzernachricht gekennzeichnet. Umgekehrt kann nichts aus Server-Ausgabe
  als eingereihte Nutzernachricht erscheinen; sie kommt nur aus der
  Oberfläche.
- Mehrere eingereihte Nachrichten gehen in Reihenfolge in denselben
  nächsten Request.
- Der Anbieter bekommt nie zwei aufeinanderfolgende Nachrichten mit
  derselben Rolle: Grenzen mehrere Nachrichten, die beim Anbieter als
  `user` gelten (Kommando-Ergebnis, eingereihte Nachrichten), aneinander,
  gehen sie als **eine** Nachricht dieser Rolle raus. Jede Ursprungsnachricht
  bleibt darin ein eigener, erkennbarer Teil (Anthropic: eigener
  Text-Block; OpenAI-kompatibel: durch eine Leerzeile getrennt), in
  unveränderter Reihenfolge. Nutzertext bleibt außerhalb jeder Umzäunung,
  ein Kommando-Ergebnis innerhalb. Das Zusammenfassen betrifft nur die
  Anfrage; gespeicherter Verlauf und Chat-Anzeige zeigen jede Nachricht
  einzeln.
- Endet der Turn ohne weitere Runde (KI fertig, Stopp, Fehler, Limit), wird
  die Warteschlange als **normale** neue Nachricht gesendet.
- Eingereihte Nachrichten laufen durch denselben Pfad wie normale: Redaction
  vor Speicherung und Versand, Verlauf, Kompaktierung, Drosselung, Caching —
  kein Sonderweg.
- Pro Sitzung läuft höchstens ein aktiver Chat-Turn; die Warteschlange ist
  der einzige Weg, während eines Turns Text einzuspeisen.

## 3. Prompt-Regel: sensible Daten nicht lesen

Der System-Prompt des Haupt-Chats enthält einen Absatz, sinngemäß:

> Umgang mit sensiblen Daten: Lies den Inhalt von Passwörtern, privaten
> Schlüsseln (z. B. `~/.ssh/id_*`), Tokens, `.env`-Dateien, Zertifikats-Keys
> oder ähnlichen Geheimnissen nur, wenn es wirklich unvermeidbar ist. Willst du
> nur prüfen, ob so eine Datei existiert oder befüllt ist, nutze Metadaten
> (z. B. `test -f`, `stat -c %s`, `ls -l`) statt `cat`. Musst du solche
> Dateien kopieren oder verschieben, tu das direkt auf dem Server (`cp`,
> `install -m 600`, Pipe/Umleitung), statt den Inhalt zu lesen und danach neu
> zu schreiben — so gelangt das Geheimnis nie in den Chat-Verlauf.

- Er gilt für den Haupt-Chat; Nebenaufrufe (Titel, Notiz, Zusammenfassung,
  Zweitmeinung) brauchen ihn nicht.
- Er ist eine **zusätzliche** Vorsichtsmaßnahme und ersetzt weder Redaction
  noch Bestätigung. Die Filter-Engine flaggt Lesezugriffe auf typische
  Secret-Pfade nicht gesondert.

## Akzeptanzfälle

- Stopp während des Streamings: Der Stream endet innerhalb einer Sekunde,
  keine weitere Anfrage, der Text bleibt mit „Abgebrochen".
- Stopp nach einem vollständigen Vorschlags-Block, aber vor dem Abbruchgrund:
  kein Bestätigungsdialog.
- Stopp während der Wartezeit eines Retries: keine zweite Anfrage.
- Eingabefeld ist während der Antwort nicht gesperrt; eine währenddessen
  gesendete Nachricht landet im nächsten Request als eigener Block außerhalb
  der Umzäunung, bzw. nach Turn-Ende als normale Nachricht.
- Eine eingereihte Nachricht mit Secret ist im Request an die KI redigiert;
  in der Datenbank liegt sie wie jede Nutzer-Nachricht geschützt (Spec 0036).
- Der System-Prompt enthält den Absatz zu sensiblen Daten.
