//! Generisches Register für wartende Bestätigungen (Host-Key-Trust,
//! Aktions-Freigabe) — je ein `oneshot`-Kanal pro wartendem Vorgang statt
//! Busy-Waiting/Polling (Aufgabenstellung Teil 2, Punkt 2/4). Ein
//! gemeinsamer generischer Typ statt zweier fast identischer
//! Kopien für `HostKeyUserDecision`/`ActionUserDecision`.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;

use tokio::sync::oneshot;

pub struct ConfirmationRegistry<K, T> {
    pending: Mutex<HashMap<K, oneshot::Sender<T>>>,
}

impl<K: Eq + Hash, T> Default for ConfirmationRegistry<K, T> {
    fn default() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }
}

impl<K: Eq + Hash, T> ConfirmationRegistry<K, T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registriert `key` als wartend und liefert den `Receiver`, auf den
    /// der Aufrufer awaitet. Ein evtl. bereits vorhandener Eintrag für
    /// denselben `key` wird ersetzt (dessen `Sender` wird gedroppt, der
    /// dortige `.await` bricht mit einem `RecvError` ab) — kann bei
    /// `connect()`-Retries nach `trust()` vorkommen (neuer Verbindungs-
    /// versuch registriert unter derselben `SessionId` neu).
    pub fn register(&self, key: K) -> oneshot::Receiver<T> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(key, tx);
        rx
    }

    /// Löst die wartende Bestätigung für `key` auf. Fehler, falls keine
    /// Bestätigung (mehr) aussteht — z. B. doppelter `respond_to_action`-
    /// Aufruf für dieselbe `action_id`, oder der wartende Vorgang wurde
    /// bereits anderweitig beendet (Session getrennt, App beendet).
    pub fn resolve(&self, key: &K, value: T) -> Result<(), String> {
        let sender = self
            .pending
            .lock()
            .unwrap()
            .remove(key)
            .ok_or_else(|| "keine wartende Bestätigung für diese ID gefunden".to_string())?;
        sender
            .send(value)
            .map_err(|_| "der wartende Vorgang wurde bereits beendet".to_string())
    }

    /// Entfernt `key` aus der Warteliste, ohne eine Bestätigung zu senden —
    /// für den Fall, dass der WARTENDE selbst aufgibt (Spec 0046, Fund 4:
    /// ein Backend-Timeout auf einen `Receiver`, den niemand mehr abwartet,
    /// sobald diese Funktion zurückkehrt), statt dass ein Antwortender
    /// (`resolve`) den Eintrag abräumt. Ohne dieses aktive Aufräumen bliebe
    /// der `Sender` verwaist in der Map — kein Speicherleck in dem Sinne,
    /// dass irgendetwas wächst, aber ein toter Eintrag, der nie wieder
    /// gebraucht wird. Best-effort: fehlt der Eintrag bereits (z. B. eine
    /// seltene Race mit einem gleichzeitigen `resolve()`), passiert
    /// nichts.
    ///
    /// **Vorsicht bei Schlüsseln, die erneut registriert werden können**
    /// (spec-reviewer-Fund, Review dieses Schritts): `register()`s eigener
    /// Kommentar oben beschreibt genau diesen Fall für die Host-Key-
    /// Registry (`SessionId`, `connect()`-Retry registriert unter
    /// demselben Schlüssel neu). Ein `cancel(key)`, das nach so einer
    /// Neu-Registrierung noch für den ALTEN Wartenden ausgeführt wird,
    /// würde den NEUEN Eintrag entfernen, ohne dass dessen `Receiver`
    /// davon weiß — der neue Wartende würde dann selbst ewig hängen.
    /// Aktuell (Spec 0046, Fund 4) ruft nur die `ActionId`-Instanz dieser
    /// Registry `cancel()` auf, wo `action_id`s nie wiederverwendet
    /// werden — kein betroffener Aufrufer heute. Vor einer Wiederverwendung
    /// auf einem Schlüsseltyp mit Neu-Registrierung (z. B. Host-Key/
    /// `SessionId`) erst prüfen, ob eine Identitätsprüfung (z. B. über
    /// einen bei `register()` mit ausgegebenen Generation-Zähler) nötig
    /// wird.
    pub fn cancel(&self, key: &K) {
        self.pending.lock().unwrap().remove(key);
    }

    /// Nur für Tests: ob für `key` noch ein wartender Eintrag existiert —
    /// unterscheidet "aktiv abgeräumt" (`cancel`) von "der `Receiver` wurde
    /// nur beiläufig gedroppt" (beides lässt ein späteres `resolve()`
    /// gleich fehlschlagen, s. Spec 0046 Fund 4, aber nur Ersteres räumt
    /// den Eintrag selbst aus der Map).
    #[cfg(test)]
    pub fn contains(&self, key: &K) -> bool {
        self.pending.lock().unwrap().contains_key(key)
    }
}
