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
}
