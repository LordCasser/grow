//! A monotone map from recorded time to presentation time.
//!
//! Only gaps supported by the captured activity evidence may be compressed.
//! The map never edits event timestamps or their order.

use std::ops::Range;

const IDLE_THRESHOLD_MS: u64 = 30_000;
const GAP_PLAYBACK_MS: f64 = 1_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GapKind {
    Idle,
    RecoveryEstimate,
}

#[derive(Debug, Clone)]
pub(super) struct Gap {
    pub source: Range<u64>,
    pub playback: Range<f64>,
    pub kind: GapKind,
    saved_before: f64,
}

#[derive(Debug, Clone)]
pub(super) struct TimeMap {
    pub origin: Option<u64>,
    pub end: u64,
    pub gaps: Vec<Gap>,
    pub playback_end: f64,
}

fn merged(mut spans: Vec<Range<u64>>) -> Vec<Range<u64>> {
    spans.sort_by_key(|range| (range.start, range.end));
    let mut result: Vec<Range<u64>> = Vec::new();
    for span in spans {
        if let Some(last) = result.last_mut()
            && span.start <= last.end
        {
            last.end = last.end.max(span.end);
        } else {
            result.push(span);
        }
    }
    result
}

fn uncovered(span: Range<u64>, protected: &[Range<u64>]) -> Vec<Range<u64>> {
    let mut cursor = span.start;
    let mut result = Vec::new();
    for cover in protected {
        if cover.start >= span.end {
            break;
        }
        if cover.end <= cursor {
            continue;
        }
        if cover.start > cursor {
            result.push(cursor..cover.start.min(span.end));
        }
        cursor = cursor.max(cover.end).min(span.end);
        if cursor >= span.end {
            break;
        }
    }
    if cursor < span.end {
        result.push(cursor..span.end);
    }
    result
}

impl TimeMap {
    /// `markers` are instants with meaningful persisted activity (for example
    /// input or visible work), not Goal/status heartbeat updates. Metadata
    /// still has a due time in the resulting map and is never dropped.
    pub fn new(
        mut markers: Vec<u64>,
        protected: Vec<Range<u64>>,
        recovery: Vec<Range<u64>>,
    ) -> Self {
        if markers.is_empty() {
            return Self {
                origin: None,
                end: 0,
                gaps: Vec::new(),
                playback_end: 0.0,
            };
        }
        markers.sort_unstable();
        markers.dedup();
        let origin = markers[0];
        let end = *markers.last().expect("nonempty markers");
        let protected = merged(protected);
        let recovery = merged(recovery);
        let mut candidates = Vec::<(Range<u64>, GapKind)>::new();
        let mut protected_index = 0;
        let mut recovery_index = 0;
        for edge in markers.windows(2) {
            while protected_index < protected.len() && protected[protected_index].end <= edge[0] {
                protected_index += 1;
            }
            for free in uncovered(edge[0]..edge[1], &protected[protected_index..]) {
                while recovery_index < recovery.len() && recovery[recovery_index].end <= free.start
                {
                    recovery_index += 1;
                }
                let mut cursor = free.start;
                for uncertain in &recovery[recovery_index..] {
                    if uncertain.start >= free.end {
                        break;
                    }
                    if uncertain.end <= cursor {
                        continue;
                    }
                    if cursor < uncertain.start {
                        let before = cursor..uncertain.start.min(free.end);
                        if before.end - before.start > IDLE_THRESHOLD_MS {
                            candidates.push((before, GapKind::Idle));
                        }
                    }
                    let overlap = cursor.max(uncertain.start)..free.end.min(uncertain.end);
                    if overlap.end > overlap.start {
                        if overlap.end - overlap.start > IDLE_THRESHOLD_MS {
                            candidates.push((overlap.clone(), GapKind::RecoveryEstimate));
                        }
                        cursor = overlap.end;
                    }
                    if cursor >= free.end {
                        break;
                    }
                }
                if cursor < free.end && free.end - cursor > IDLE_THRESHOLD_MS {
                    candidates.push((cursor..free.end, GapKind::Idle));
                }
            }
        }
        candidates.sort_by_key(|(span, _)| (span.start, span.end));
        let mut gaps = Vec::with_capacity(candidates.len());
        let mut saved = 0.0;
        for (source, kind) in candidates {
            let playback_start = source.start.saturating_sub(origin) as f64 - saved;
            let playback_end = playback_start + GAP_PLAYBACK_MS;
            let saved_before = saved;
            saved += (source.end - source.start) as f64 - GAP_PLAYBACK_MS;
            gaps.push(Gap {
                source,
                playback: playback_start..playback_end,
                kind,
                saved_before,
            });
        }
        Self {
            origin: Some(origin),
            end,
            gaps,
            playback_end: end.saturating_sub(origin) as f64 - saved,
        }
    }

    pub fn to_playback(&self, source: u64) -> f64 {
        let Some(origin) = self.origin else {
            return 0.0;
        };
        let completed = self.gaps.partition_point(|gap| gap.source.end <= source);
        if let Some(gap) = self
            .gaps
            .get(completed)
            .filter(|gap| source >= gap.source.start)
        {
            let fraction =
                (source - gap.source.start) as f64 / (gap.source.end - gap.source.start) as f64;
            return gap.playback.start + fraction * GAP_PLAYBACK_MS;
        }
        let saved = self.gaps[..completed].last().map_or(0.0, |gap| {
            gap.saved_before + (gap.source.end - gap.source.start) as f64 - GAP_PLAYBACK_MS
        });
        source.saturating_sub(origin) as f64 - saved
    }

    pub fn to_source(&self, playback: f64) -> Option<u64> {
        let origin = self.origin?;
        let completed = self
            .gaps
            .partition_point(|gap| gap.playback.end <= playback);
        if let Some(gap) = self
            .gaps
            .get(completed)
            .filter(|gap| playback >= gap.playback.start)
        {
            let fraction = (playback - gap.playback.start) / GAP_PLAYBACK_MS;
            return Some(
                gap.source.start + (fraction * (gap.source.end - gap.source.start) as f64) as u64,
            );
        }
        let saved = self.gaps[..completed].last().map_or(0.0, |gap| {
            gap.saved_before + (gap.source.end - gap.source.start) as f64 - GAP_PLAYBACK_MS
        });
        Some(
            origin
                .saturating_add((playback.max(0.0) + saved) as u64)
                .min(self.end),
        )
    }

    pub fn crossed_gap(&self, previous: f64, current: f64) -> Option<&Gap> {
        let crossed = self
            .gaps
            .partition_point(|gap| gap.playback.start < current);
        crossed
            .checked_sub(1)
            .and_then(|index| self.gaps.get(index))
            .filter(|gap| previous <= gap.playback.start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_threshold_and_inverse_mapping() {
        let map = TimeMap::new(vec![1_000, 31_000, 62_000], vec![], vec![]);
        assert_eq!(map.gaps.len(), 1);
        assert_eq!(map.gaps[0].source, 31_000..62_000);
        assert_eq!(map.to_playback(31_000), 30_000.0);
        assert_eq!(map.to_playback(62_000), 31_000.0);
        assert_eq!(map.to_source(30_500.0), Some(46_500));
    }

    #[test]
    fn execution_in_another_node_blocks_idle() {
        let map = TimeMap::new(vec![0, 100_000], vec![10_000..90_000], vec![]);
        assert_eq!(map.gaps.len(), 0);
    }

    #[test]
    fn recovery_is_visibly_estimated() {
        let map = TimeMap::new(vec![0, 3_600_000], vec![], vec![0..3_600_000]);
        assert_eq!(map.gaps[0].kind, GapKind::RecoveryEstimate);
        assert_eq!(map.playback_end, 1_000.0);
    }

    #[test]
    fn first_gap_reports_once_after_playback_starts() {
        let map = TimeMap::new(vec![1_000, 61_000], vec![], vec![]);
        assert!(map.crossed_gap(0.0, 0.0).is_none());
        assert!(map.crossed_gap(0.0, 0.1).is_some());
        assert!(map.crossed_gap(0.1, 100.0).is_none());
    }

    #[test]
    fn many_gaps_preserve_order_and_inverse_mapping() {
        let markers = (0..1000).map(|index| index * 60_000).collect::<Vec<_>>();
        let map = TimeMap::new(markers.clone(), vec![], vec![]);
        assert_eq!(map.gaps.len(), 999);
        for (index, source) in markers.into_iter().enumerate() {
            let playback = index as f64 * GAP_PLAYBACK_MS;
            assert_eq!(map.to_playback(source), playback);
            assert_eq!(map.to_source(playback), Some(source));
        }
        assert!(map.crossed_gap(998_000.1, 999_000.0).is_none());
    }
}
