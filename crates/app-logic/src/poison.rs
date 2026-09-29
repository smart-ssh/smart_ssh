//! Spec 0088, A2.1/A2.2: Sperren, die eine Vergiftung überstehen.
//!
//! Ein `std::sync::Mutex` gilt als **vergiftet**, sobald irgendein Thread mit
//! gehaltener Sperre panickt. Jeder spätere `lock().unwrap()` panickt dann
//! ebenfalls — aus einem einmaligen Fehler wird ein dauerhafter. Für die
//! kurzen, in sich abgeschlossenen Zuweisungen hinter
//! [`crate::session::Session::pending_action`] und
//! [`crate::session::Session::mcp_origin_flags`] ist das die falsche
//! Reaktion: Der geschützte Wert ist ein einfaches `Option` bzw. ein
//! `Vec<bool>`, beide nach jeder einzelnen Operation in sich stimmig. Es
//! gibt keinen halbfertigen Zwischenzustand, vor dem eine Vergiftung
//! schützen könnte.
//!
//! Besonders im `Drop` eines Wertes (Spec 0088, A2.2): Ein Panic dort,
//! während bereits ein anderer Panic abgewickelt wird, bricht den **Prozess**
//! ab — die Sitzung wäre nicht bloß gestört, die App wäre weg.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Sperrt `mutex` und arbeitet auch mit einem vergifteten Wert weiter,
/// statt zu panicken (Spec 0088, A2.1).
///
/// Bewusst kein `unwrap`/`expect`: `PoisonError::into_inner` liefert genau
/// denselben Guard, den ein erfolgreicher `lock()` geliefert hätte.
pub(crate) fn lock_tolerating_poison<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Vergiftet `mutex` absichtlich — ein Thread panickt mit gehaltener Sperre.
///
/// Nur für Tests (Spec 0088, T5/T6a/T6b/T6c/T7): Die Vergiftung ist der
/// Zustand, gegen den A2.1/A2.2 absichern, und sie lässt sich anders nicht
/// herstellen. Der Panic des Hilfsthreads erscheint als Rauschen in der
/// Testausgabe — das ist gewollt und kein Fehlschlag.
#[cfg(test)]
pub(crate) fn poison_for_test<T: Send>(mutex: &Mutex<T>) {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let _guard = mutex.lock().expect("noch nicht vergiftet");
                panic!("Testvergiftung (Spec 0088) — erwartet");
            })
            .join()
            .expect_err("der Hilfsthread muss gepanickt haben");
    });
    assert!(
        mutex.lock().is_err(),
        "der Mutex muss nach dem Panic vergiftet sein"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gegenbeweis-Anker für A2.1: Gegen ein `lock().unwrap()` wäre dieser
    /// Test rot (Panic statt Rückgabe).
    #[test]
    fn test_lock_tolerating_poison_still_yields_the_value_after_poisoning() {
        let mutex = Mutex::new(41_u32);
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _guard = mutex.lock().expect("frischer Mutex ist nicht vergiftet");
                    panic!("vergiftet den Mutex absichtlich");
                })
                .join()
                .expect_err("der Thread muss gepanickt haben");
        });
        assert!(mutex.lock().is_err(), "Mutex muss jetzt vergiftet sein");

        *lock_tolerating_poison(&mutex) += 1;
        assert_eq!(*lock_tolerating_poison(&mutex), 42);
    }
}
