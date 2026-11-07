//! Issue #1193: the event that tells the frontend the elevated file-browser
//! channel was switched off by the backend after inactivity.

use serde::Serialize;

use crate::event_names as names;
use crate::events::EventEmitter;
use crate::state::SessionId;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub(crate) struct ElevationAutoDisabledPayload {
    session_id: SessionId,
    minutes: u32,
}

/// Issue #1194: the channel was revoked because the server profile no longer
/// allows elevated rights.
pub fn emit_elevation_revoked_by_profile(emitter: &dyn EventEmitter, session_id: SessionId) {
    crate::events::emit_session_only(emitter, names::ELEVATION_REVOKED_BY_PROFILE, session_id);
}

/// The channel was switched off after `minutes` without elevated activity.
pub fn emit_elevation_auto_disabled(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    minutes: u32,
) {
    let payload = ElevationAutoDisabledPayload {
        session_id,
        minutes,
    };
    match serde_json::to_value(&payload) {
        Ok(value) => emitter.emit_event(names::ELEVATION_AUTO_DISABLED, value),
        Err(err) => eprintln!(
            "Event '{}' nicht serialisierbar: {err}",
            names::ELEVATION_AUTO_DISABLED
        ),
    }
}
