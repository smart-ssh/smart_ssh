//! Device time zone lookup (used for the time zone of activity events).

/// Source of the device's IANA time zone name; `None` when unknown.
pub type TimeZoneSource = std::sync::Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Default source: the platform's zone; a failed or empty lookup is `None`.
pub fn device_time_zone_source() -> TimeZoneSource {
    std::sync::Arc::new(|| {
        iana_time_zone::get_timezone()
            .ok()
            .filter(|z| !z.trim().is_empty())
    })
}
