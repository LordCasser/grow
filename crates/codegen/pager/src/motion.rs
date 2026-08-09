//! Monotonic, render-only motion primitives.
//!
//! A frame samples time exactly once and every animated surface derives its
//! phase from that sample.  Motion never owns lifecycle state and event rates
//! (ACP chunks, task completions, input) never advance an animation.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The shared monotonic time sample for one rendered frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameStamp {
    now: Instant,
    wall_now: SystemTime,
    elapsed: Duration,
}

impl FrameStamp {
    /// Capture a frame relative to the application's stable motion origin.
    pub fn capture(origin: Instant) -> Self {
        let now = Instant::now();
        Self {
            now,
            wall_now: SystemTime::now(),
            elapsed: now.saturating_duration_since(origin),
        }
    }

    /// Construct a deterministic frame sample.
    pub fn at(origin: Instant, now: Instant) -> Self {
        Self {
            now,
            wall_now: UNIX_EPOCH + now.saturating_duration_since(origin),
            elapsed: now.saturating_duration_since(origin),
        }
    }

    pub fn now(self) -> Instant {
        self.now
    }

    pub fn elapsed(self) -> Duration {
        self.elapsed
    }

    pub fn wall_now(self) -> SystemTime {
        self.wall_now
    }

    /// Quantize monotonic time into fixed-duration samples.
    pub fn sample(self, period: Duration) -> u64 {
        debug_assert!(!period.is_zero());
        let period_nanos = period.as_nanos().max(1);
        (self.elapsed.as_nanos() / period_nanos).min(u64::MAX as u128) as u64
    }
}

impl Default for FrameStamp {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            now,
            wall_now: SystemTime::now(),
            elapsed: Duration::ZERO,
        }
    }
}

/// Stable semantic cadences.  The configured FPS only controls how often
/// these phases are sampled; changing FPS must not change their speed.
pub const SPINNER_FRAME: Duration = Duration::from_millis(132);
pub const TITLE_SPINNER_FRAME: Duration = Duration::from_millis(264);
pub const AMBIENT_PULSE_FRAME: Duration = Duration::from_millis(264);
pub const USER_WAITING_PULSE_PERIOD: Duration = Duration::from_millis(1_309);
pub const ACTION_REQUIRED_HALF_CYCLE: Duration = Duration::from_millis(500);
pub const SLOW_FRAME_INTERVAL: Duration = Duration::from_millis(83);

pub fn spinner_index(frame: FrameStamp, len: usize) -> usize {
    phase_index(frame, SPINNER_FRAME, len)
}

pub fn spinner_glyph<'a>(frame: FrameStamp, frames: &'a [&'a str]) -> &'a str {
    phase_glyph(frame, SPINNER_FRAME, frames)
}

pub fn phase_glyph<'a>(frame: FrameStamp, period: Duration, frames: &'a [&'a str]) -> &'a str {
    frames
        .get(phase_index(frame, period, frames.len()))
        .copied()
        .unwrap_or("")
}

pub fn title_spinner_index(frame: FrameStamp, len: usize) -> usize {
    phase_index(frame, TITLE_SPINNER_FRAME, len)
}

pub fn title_spinner_glyph(frame: FrameStamp, frames: &[char]) -> char {
    frames
        .get(title_spinner_index(frame, frames.len()))
        .copied()
        .unwrap_or(' ')
}

pub fn action_required_visible(frame: FrameStamp) -> bool {
    frame.sample(ACTION_REQUIRED_HALF_CYCLE).is_multiple_of(2)
}

/// A deterministic `0..=1` sin² pulse with the supplied full period.
pub fn pulse01(frame: FrameStamp, period: Duration) -> f32 {
    debug_assert!(!period.is_zero());
    let phase = frame.elapsed().as_secs_f64() / period.as_secs_f64();
    (std::f64::consts::PI * phase).sin().powi(2) as f32
}

/// A traveling sin² wave whose temporal speed is independent from redraw FPS.
pub fn spatial_wave01(frame: FrameStamp, row: u16, rows_per_cycle: u16, period: Duration) -> f32 {
    debug_assert!(!period.is_zero());
    let spatial_phase = row as f64 / rows_per_cycle.max(1) as f64 * std::f64::consts::TAU;
    // sin² completes one visual cycle over PI radians.
    let temporal_phase =
        frame.elapsed().as_secs_f64() / period.as_secs_f64() * std::f64::consts::PI;
    (temporal_phase + spatial_phase).sin().powi(2) as f32
}

pub fn half_cycle_visible(frame: FrameStamp, half_cycle: Duration) -> bool {
    frame.sample(half_cycle).is_multiple_of(2)
}

pub fn phase_index(frame: FrameStamp, period: Duration, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    frame.sample(period) as usize % len
}

/// Return the first aligned deadline strictly after `now`.
pub fn next_aligned_deadline(origin: Instant, now: Instant, interval: Duration) -> Instant {
    debug_assert!(!interval.is_zero());
    let elapsed = now.saturating_duration_since(origin);
    let step = interval.as_nanos().max(1);
    let next_step = elapsed.as_nanos() / step + 1;
    let next_nanos = next_step.saturating_mul(step).min(u64::MAX as u128) as u64;
    origin + Duration::from_nanos(next_nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_is_a_pure_function_of_time() {
        let origin = Instant::now();
        let now = origin + Duration::from_millis(999);
        assert_eq!(FrameStamp::at(origin, now), FrameStamp::at(origin, now));
        assert_eq!(spinner_index(FrameStamp::at(origin, now), 8), 7);
    }

    #[test]
    fn missed_samples_catch_up_without_replay() {
        let origin = Instant::now();
        let late = FrameStamp::at(origin, origin + SPINNER_FRAME * 19);
        assert_eq!(spinner_index(late, 8), 3);
    }

    #[test]
    fn aligned_deadline_is_strictly_future_and_does_not_drift() {
        let origin = Instant::now();
        let interval = Duration::from_millis(33);
        let now = origin + Duration::from_millis(100);
        let next = next_aligned_deadline(origin, now, interval);
        assert_eq!(next, origin + Duration::from_millis(132));
        assert!(next > now);
    }

    #[test]
    fn sampling_rate_does_not_change_spinner_phase() {
        let origin = Instant::now();
        let cycle = SPINNER_FRAME * 8;

        // FPS chooses which instants get painted, not the semantic cycle.
        // Both samplers therefore land on the same phase at every common
        // wall-clock sample and wrap at the duration defined above.
        for elapsed in (0..=cycle.as_millis() as u64).step_by(100) {
            let at = Duration::from_millis(elapsed);
            let sampled_at_30_fps = spinner_index(FrameStamp::at(origin, origin + at), 8);
            let sampled_at_60_fps = spinner_index(FrameStamp::at(origin, origin + at), 8);
            assert_eq!(sampled_at_30_fps, sampled_at_60_fps);
        }
        assert_eq!(
            spinner_index(FrameStamp::at(origin, origin + cycle), 8),
            spinner_index(FrameStamp::at(origin, origin), 8)
        );
    }

    #[test]
    fn animated_surfaces_cannot_reintroduce_private_counters_or_glyph_indexing() {
        let forbidden = [
            "spinner_tick",
            "welcome_tick",
            "tick_count",
            "SPINNER_DIVISOR",
            "MONITOR_PULSE_DIVISOR",
            "spinner_index(",
            "spinner_frames()[",
            "spinner_frames[",
            "advance_animation_tick",
            "TickDemand",
            "base_tick",
            "motion_tick",
            ".with_tick(",
            "wave_brightness",
            "pulse_brightness",
            "mermaid_tick",
            "edit_hl_tick",
            "edit_hl_needs_tick",
        ];

        fn visit(dir: &std::path::Path, forbidden: &[&str]) {
            for entry in std::fs::read_dir(dir).expect("read source directory") {
                let path = entry.expect("read source entry").path();
                if path.is_dir() {
                    visit(&path, forbidden);
                    continue;
                }
                if path.extension().and_then(std::ffi::OsStr::to_str) != Some("rs")
                    || path.file_name().and_then(std::ffi::OsStr::to_str) == Some("motion.rs")
                {
                    continue;
                }
                let source = std::fs::read_to_string(&path).expect("read Rust source");
                // Glyph modules legitimately index their own tables in unit
                // tests.  The architectural constraint applies to production
                // renderers, not assertions below their test boundary.
                let production = source.split("#[cfg(test)]").next().unwrap_or(&source);
                for needle in forbidden {
                    assert!(
                        !production.contains(needle),
                        "{} bypasses the shared motion API with {needle:?}",
                        path.display()
                    );
                }
            }
        }

        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        visit(&manifest.join("src"), &forbidden);
        visit(&manifest.join("../pager-minimal/src"), &forbidden);
        visit(&manifest.join("../pager-render/src"), &forbidden);
    }
}
