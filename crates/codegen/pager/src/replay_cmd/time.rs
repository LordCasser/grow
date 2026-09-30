use chrono::{FixedOffset, Local, TimeZone, Utc};

pub(super) fn format_recorded_time(timestamp_ms: u64) -> Option<String> {
    format_recorded_time_with(timestamp_ms, |timestamp| {
        Local
            .timestamp_millis_opt(timestamp)
            .single()
            .map(|date| *date.offset())
    })
}

fn format_recorded_time_with(
    timestamp_ms: u64,
    resolve_offset: impl FnOnce(i64) -> Option<FixedOffset>,
) -> Option<String> {
    let timestamp = i64::try_from(timestamp_ms).ok()?;
    let utc = Utc.timestamp_millis_opt(timestamp).single()?;
    let offset = resolve_offset(timestamp);

    Some(match offset {
        Some(offset) if offset.local_minus_utc() != 0 => utc
            .with_timezone(&offset)
            .format("%Y-%m-%d %H:%M:%S UTC%:z")
            .to_string(),
        _ => utc.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_resolved_local_offset_or_utc_fallback() {
        assert_eq!(
            format_recorded_time_with(0, |_| FixedOffset::east_opt(8 * 3600)),
            Some("1970-01-01 08:00:00 UTC+08:00".to_owned())
        );
        assert_eq!(
            format_recorded_time_with(0, |_| FixedOffset::west_opt(5 * 3600)),
            Some("1969-12-31 19:00:00 UTC-05:00".to_owned())
        );
        assert_eq!(
            format_recorded_time_with(0, |_| None),
            Some("1970-01-01 00:00:00 UTC".to_owned())
        );
        assert_eq!(
            format_recorded_time_with(0, |_| FixedOffset::east_opt(0)),
            Some("1970-01-01 00:00:00 UTC".to_owned())
        );
        assert_eq!(format_recorded_time_with(u64::MAX, |_| None), None);
    }

    #[test]
    fn resolves_offset_for_each_historical_instant_across_dst_and_repeated_hour() {
        // 2024-11-03 05:30 UTC and 06:30 UTC are both 01:30 in New York,
        // but fall on opposite sides of the DST transition.
        let daylight = Utc.with_ymd_and_hms(2024, 11, 3, 5, 30, 0).unwrap();
        let standard = Utc.with_ymd_and_hms(2024, 11, 3, 6, 30, 0).unwrap();
        let daylight_ms = daylight.timestamp_millis() as u64;
        let standard_ms = standard.timestamp_millis() as u64;

        assert_eq!(
            format_recorded_time_with(daylight_ms, |_| FixedOffset::west_opt(4 * 3600)),
            Some("2024-11-03 01:30:00 UTC-04:00".to_owned())
        );
        assert_eq!(
            format_recorded_time_with(standard_ms, |_| FixedOffset::west_opt(5 * 3600)),
            Some("2024-11-03 01:30:00 UTC-05:00".to_owned())
        );
    }
}
