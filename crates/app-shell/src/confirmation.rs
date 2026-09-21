//! Generisches Register für wartende Bestätigungen (Host-Key-Trust,
//! Aktions-Freigabe) — je ein `oneshot`-Kanal pro wartendem Vorgang statt
//! Busy-Waiting/Polling (Aufgabenstellung Teil 2, Punkt 2/4). Ein
//! gemeinsamer generischer Typ statt zweier fast identischer
//! Kopien für `HostKeyUserDecision`/`ActionUserDecision`.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use tokio::sync::oneshot;

/// Spec 0068, Teil 5b: Identität EINER Registrierung. Derselbe Schlüssel
/// kann neu registriert werden (Host-Key: `connect()`-Retry unter derselben
/// `SessionId`); wer aufgibt, räumt über [`ConfirmationRegistry::
/// cancel_if_current`] nur den Eintrag mit SEINER Generation ab, nie einen
/// inzwischen neu registrierten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistrationGeneration(u64);

pub struct ConfirmationRegistry<K, T> {
    pending: Mutex<HashMap<K, (RegistrationGeneration, oneshot::Sender<T>)>>,
    next_generation: AtomicU64,
}

impl<K: Eq + Hash, T> Default for ConfirmationRegistry<K, T> {
    fn default() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(0),
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
        self.register_tracked(key).1
    }

    /// Wie [`Self::register`], liefert zusätzlich die Generation dieser
    /// Registrierung für [`Self::cancel_if_current`] (Spec 0068, Teil 5b).
    pub fn register_tracked(&self, key: K) -> (RegistrationGeneration, oneshot::Receiver<T>) {
        let (tx, rx) = oneshot::channel();
        let generation =
            RegistrationGeneration(self.next_generation.fetch_add(1, Ordering::Relaxed));
        self.pending.lock().unwrap().insert(key, (generation, tx));
        (generation, rx)
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
            .map(|(_, sender)| sender)
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
    /// wird. → Genau das ist [`Self::cancel_if_current`] (Spec 0068,
    /// Teil 5b); die Host-Key-Registry nutzt nur diese Variante.
    pub fn cancel(&self, key: &K) {
        self.pending.lock().unwrap().remove(key);
    }

    /// Spec 0068, Teil 5b: entfernt `key` nur, wenn der Eintrag noch aus
    /// der Registrierung `generation` stammt. Ein inzwischen (z. B. durch
    /// einen `connect()`-Retry) neu registrierter Eintrag bleibt unberührt.
    /// Liefert, ob etwas entfernt wurde.
    pub fn cancel_if_current(&self, key: &K, generation: RegistrationGeneration) -> bool {
        let mut pending = self.pending.lock().unwrap();
        if pending
            .get(key)
            .is_some_and(|(current, _)| *current == generation)
        {
            pending.remove(key);
            true
        } else {
            false
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec 0068, Teil 5b — die bekannte Falle: gibt ein ALTER Wartender
    /// auf, nachdem derselbe Schlüssel neu registriert wurde, darf sein
    /// Aufräumen den NEUEN Eintrag nicht entfernen.
    #[tokio::test]
    async fn test_cancel_if_current_never_removes_a_re_registered_entry() {
        let registry: ConfirmationRegistry<u32, &'static str> = ConfirmationRegistry::new();
        let (old_generation, _old_rx) = registry.register_tracked(7);
        let (new_generation, new_rx) = registry.register_tracked(7);
        assert_ne!(old_generation, new_generation);

        assert!(!registry.cancel_if_current(&7, old_generation));
        assert!(registry.contains(&7), "neuer Eintrag muss bleiben");

        registry
            .resolve(&7, "trust")
            .expect("neuer Wartender muss erreichbar bleiben");
        let value = tokio::time::timeout(std::time::Duration::from_secs(5), new_rx)
            .await
            .expect("darf nicht hängen")
            .expect("Sender darf nicht gedroppt sein");
        assert_eq!(value, "trust");
    }

    #[test]
    fn test_cancel_if_current_removes_its_own_entry() {
        let registry: ConfirmationRegistry<u32, ()> = ConfirmationRegistry::new();
        let (generation, _rx) = registry.register_tracked(7);
        assert!(registry.cancel_if_current(&7, generation));
        assert!(!registry.contains(&7));
        assert!(registry.resolve(&7, ()).is_err());
    }
}
