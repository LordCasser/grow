//! Logo component — renders the Grow Braille wordmark.
//!
//! Two assets, measured at render time by the helpers below:
//! - [`LOGO`] (`grow-big.txt`): 80 cols × 35 rows — the large wordmark.
//! - [`LOGO_SMALL`] (`grow-small.txt`): 50 cols × 22 rows — the small wordmark.
//!
//! Each slot can be overridden once per process by `$GROW_HOME/logo/big.txt`
//! or `$GROW_HOME/logo/small.txt`; invalid, unreadable and empty files fall
//! back independently to the compiled assets.
//!
//! [`pick_logo`] tiers by **both** width and height (the stacked gate screens
//! need a width dimension too, so a tall-but-narrow terminal no longer gets a
//! logo that overflows). The side-by-side hero tiers live in [`super::hero`],
//! which sizes its columns from the same asset extents.

use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::render::color::blend_color;
use crate::theme::Theme;

pub(crate) const LOGO: &str = include_str!("../../../assets/logo/grow-big.txt");
pub(crate) const LOGO_SMALL: &str = include_str!("../../../assets/logo/grow-small.txt");

/// The two runtime logo slots. The built-in assets remain the per-slot
/// fallback, so a missing or invalid user file never removes the logo.
#[derive(Debug)]
struct LogoAssets {
    big: String,
    small: String,
}

impl LogoAssets {
    fn art(&self, size: LogoSize) -> &str {
        match size {
            LogoSize::Big => &self.big,
            LogoSize::Small => &self.small,
        }
    }
}

/// User overrides are intentionally loaded once per process. This keeps the
/// render path free of filesystem I/O and makes a logo change take effect on
/// the next process start, not halfway through an active frame.
static USER_LOGOS: OnceLock<LogoAssets> = OnceLock::new();
const MAX_LOGO_BYTES: u64 = 1024 * 1024;

fn assets() -> &'static LogoAssets {
    // Unit tests exercise deterministic built-in geometry even when the
    // developer has a personal override under ~/.grow. Production builds
    // retain the real one-time user lookup (including PTY/integration tests).
    #[cfg(test)]
    {
        USER_LOGOS.get_or_init(|| LogoAssets {
            big: LOGO.to_owned(),
            small: LOGO_SMALL.to_owned(),
        })
    }
    #[cfg(not(test))]
    {
        USER_LOGOS.get_or_init(|| load_logo_assets_from_dir(&crate::util::grow_home().join("logo")))
    }
}

/// Load the two override slots from an explicit directory.
///
/// This is kept as a pure-input boundary for tests: filesystem failures,
/// invalid UTF-8 and empty files are all handled as a per-slot fallback to the
/// compiled assets. The caller owns the directory and no global state is
/// touched here.
fn load_logo_assets_from_dir(dir: &Path) -> LogoAssets {
    LogoAssets {
        big: load_logo_slot(dir, "big.txt", LOGO),
        small: load_logo_slot(dir, "small.txt", LOGO_SMALL),
    }
}

fn load_logo_slot(dir: &Path, name: &str, builtin: &str) -> String {
    let mut file = std::fs::File::open(dir.join(name)).ok();
    let bytes = file.as_mut().and_then(|file| {
        let mut bytes = Vec::new();
        file.take(MAX_LOGO_BYTES + 1).read_to_end(&mut bytes).ok()?;
        (bytes.len() as u64 <= MAX_LOGO_BYTES).then_some(bytes)
    });
    bytes
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| normalize_logo(&text))
        .unwrap_or_else(|| builtin.to_owned())
}

/// Normalize only the line boundaries that cannot contribute visible art.
/// Interior blank rows are retained, while CRLF/CR are normalized so `\r`
/// never reaches the terminal renderer.
fn normalize_logo(text: &str) -> Option<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.chars().any(|ch| ch != '\n' && ch.is_control())
        || !normalized.chars().any(|ch| {
            !ch.is_whitespace()
                && unicode_width::UnicodeWidthChar::width(ch).is_some_and(|width| width > 0)
        })
    {
        return None;
    }
    let mut lines: Vec<&str> = normalized.split('\n').collect();
    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// The two logo tiers (big wordmark / small wordmark), with their measured
/// asset extents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogoSize {
    Big,
    Small,
}

impl LogoSize {
    pub(crate) fn art(self) -> &'static str {
        assets().art(self)
    }

    /// Width (cols) of the art.
    pub(crate) fn width(self) -> u16 {
        visual_width(self.art())
    }

    /// Height (rows) of the art.
    pub(crate) fn height(self) -> u16 {
        count_lines(self.art())
    }
}

/// Horizontal padding (cols) the layout reserves around the logo.
pub(super) const H_PAD: u16 = 2;
/// Vertical padding (rows) the layout reserves around the logo.
pub(super) const V_PAD: u16 = 2;
/// Minimum width (cols) of the hero text column (side-by-side layout).
pub(super) const RIGHT_COL_MIN: u16 = 59;
/// Minimum slack (rows) the stacked gate screens need below the logo (logo
/// gap + flex gap + headroom). The full chrome (menu / prompt / version /
/// error) is accounted for exactly by [`super::WelcomeLayout::compute_stacked`],
/// which drops the logo when the version row would clip.
const STACKED_CHROME: u16 = 3;

/// Asset width (cols) — maximum raw line width, matching the renderer's
/// padding-based centering.
pub(super) fn visual_width(logo: &str) -> u16 {
    logo_lines(logo)
        .filter(|l| !l.is_empty())
        .map(unicode_width::UnicodeWidthStr::width)
        .max()
        .unwrap_or(24)
        .min(u16::MAX as usize) as u16
}

/// Asset height (rows) — number of retained lines (including intentional
/// interior blank rows).
pub(super) fn count_lines(logo: &str) -> u16 {
    logo_lines(logo).count().min(u16::MAX as usize) as u16
}

fn logo_lines(logo: &str) -> impl Iterator<Item = &str> {
    logo.trim_matches(['\r', '\n']).split('\n')
}

/// Pick the largest logo that fits a **stacked** arrangement (the gate
/// screens). Requires both dimensions; the height gate includes the fixed
/// chrome below the logo so the version row stays on screen.
pub fn pick_logo(area_w: u16, area_h: u16) -> Option<&'static str> {
    let big = LogoSize::Big;
    let small = LogoSize::Small;
    if area_w >= big.width().saturating_add(2 * H_PAD)
        && area_h
            >= big
                .height()
                .saturating_add(2 * V_PAD)
                .saturating_add(STACKED_CHROME)
    {
        Some(big.art())
    } else if area_w >= small.width().saturating_add(2 * H_PAD)
        && area_h
            >= small
                .height()
                .saturating_add(2 * V_PAD)
                .saturating_add(STACKED_CHROME)
    {
        Some(small.art())
    } else {
        None
    }
}

/// Per-glyph shine opacity in `[0, 1]` at normalized diagonal position `diag`
/// (0 = bottom-left .. 1 = top-right) and animation time `secs`. A raised-cosine
/// band sweeps bottom-left → top-right and parks off-screen between sweeps; a
/// gentle global pulse breathes underneath it. 0 keeps the resting gray, 1 is
/// full bright.
fn shine_opacity(diag: f32, secs: f32) -> f32 {
    const BAND: f32 = 0.38; // half-width of the shine band — wider = more gradual falloff
    const CYCLE: f32 = 4.0; // seconds per sweep + rest
    const SWEEP_FRAC: f32 = 0.32; // portion of the cycle spent sweeping (~1.3s glint, rest idles)
    const SHINE: f32 = 0.33; // peak shine strength
    const PULSE: f32 = 0.06; // global breathing amount
    const PULSE_SECS: f32 = 5.0; // breathing period

    let p = (secs % CYCLE) / CYCLE;
    let q = (p / SWEEP_FRAC).min(1.0); // parks the band off-screen during the rest
    let band_pos = -BAND + q * (1.0 + 2.0 * BAND);
    let pulse = PULSE * (0.5 - 0.5 * (std::f32::consts::TAU * secs / PULSE_SECS).cos());

    let d = (diag - band_pos).abs();
    let shine = if d < BAND {
        0.5 * (1.0 + (std::f32::consts::PI * d / BAND).cos())
    } else {
        0.0
    };
    (pulse + SHINE * shine).clamp(0.0, 1.0)
}

fn render_into(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    logo: &str,
    frame: crate::motion::FrameStamp,
) {
    let lines: Vec<&str> = logo_lines(logo).collect();
    let rows = lines.len().max(1) as f32;
    let cols = lines
        .iter()
        .map(|l| unicode_width::UnicodeWidthStr::width(*l))
        .max()
        .unwrap_or(1)
        .max(1);
    let cols_f32 = cols as f32;
    let secs = frame.elapsed().as_secs_f32();

    // Blend each glyph from the resting gray toward the bright text color by its
    // shine opacity, so a sheen sweeps across the character art. Adjacent glyphs
    // that land on the same blended color share one Span to hold down the
    // per-frame allocation.
    let base = theme.gray;
    let hilite = theme.text_primary;
    let logo_lines: Vec<Line> = lines
        .iter()
        .enumerate()
        .map(|(row, line)| {
            let mut spans: Vec<Span> = Vec::new();
            let mut run = String::new();
            let mut run_color: Option<Color> = None;
            let line_width = unicode_width::UnicodeWidthStr::width(*line);
            let padded = format!("{line}{}", " ".repeat(cols.saturating_sub(line_width)));
            let mut display_col = 0;
            for ch in padded.chars() {
                // Sweep along the bottom-left → top-right diagonal: the
                // coordinate grows as col increases and row decreases.
                let diag = (display_col as f32 + (rows - 1.0 - row as f32)) / (cols_f32 + rows);
                let color = blend_color(base, hilite, shine_opacity(diag, secs)).unwrap_or(base);
                if run_color != Some(color) {
                    if let Some(prev) = run_color {
                        spans.push(Span::styled(
                            std::mem::take(&mut run),
                            Style::default().fg(prev),
                        ));
                    }
                    run_color = Some(color);
                }
                run.push(ch);
                display_col += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            }
            if let Some(prev) = run_color {
                spans.push(Span::styled(run, Style::default().fg(prev)));
            }
            Line::from(spans).alignment(Alignment::Center)
        })
        .collect();
    Paragraph::new(logo_lines).render(area, buf);
}

/// Rows the picked logo occupies (0 when the area is too small for any logo).
pub fn logo_line_count(area_w: u16, area_h: u16) -> u16 {
    pick_logo(area_w, area_h).map_or(0, count_lines)
}

/// Render the picked logo (stacked arrangement), centered into `area`.
pub fn render_logo(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    area_w: u16,
    area_h: u16,
    frame: crate::motion::FrameStamp,
) {
    if let Some(logo) = pick_logo(area_w, area_h) {
        render_into(area, buf, theme, logo, frame);
    }
}

/// Render a specific logo art (centered) into `area`. Used by the hero, which
/// picks the tier itself from its own side-by-side gates, and by the agent
/// empty-state (Task B), which tiers via the same asset extents.
pub(crate) fn render_logo_into(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    logo: &'static str,
    frame: crate::motion::FrameStamp,
) {
    render_into(area, buf, theme, logo, frame);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_picks_by_width_and_height() {
        // The stacked gate screens require both dimensions: big needs
        // w ≥ 84 && h ≥ 42 (35 rows + 2*V_PAD + chrome slack); small needs
        // w ≥ 54 && h ≥ 29 (22 rows + 2*V_PAD + chrome slack).
        assert_eq!(pick_logo(33, 50), None, "too narrow for any logo");
        assert_eq!(pick_logo(53, 40), None, "too narrow for the small logo");
        assert_eq!(pick_logo(54, 28), None, "too short for the small logo");
        assert_eq!(pick_logo(54, 29), Some(LOGO_SMALL));
        assert_eq!(
            pick_logo(83, 50),
            Some(LOGO_SMALL),
            "wide but not wide enough for big"
        );
        assert_eq!(
            pick_logo(84, 41),
            Some(LOGO_SMALL),
            "tall but not tall enough for big"
        );
        assert_eq!(pick_logo(84, 42), Some(LOGO));
        assert_eq!(pick_logo(150, 45), Some(LOGO));
    }

    #[test]
    fn logo_assets_are_grow_braille_emblems() {
        for logo in [LOGO, LOGO_SMALL] {
            assert!(
                logo.lines()
                    .flat_map(str::chars)
                    .all(|ch| ('\u{2800}'..='\u{28ff}').contains(&ch))
            );
        }

        assert!(LOGO.contains("⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣷⣥"));
        assert!(LOGO_SMALL.contains("⢿⣿⣿⣿⣿⣿⣿⣿⣿⡿⠃"));
        assert_eq!(visual_width(LOGO), 80);
        assert_eq!(count_lines(LOGO), 35);
        assert_eq!(visual_width(LOGO_SMALL), 50);
        assert_eq!(count_lines(LOGO_SMALL), 22);
    }

    #[test]
    fn hero_gate_constants_calibrated_to_assets() {
        // Side-by-side gates = logo extent + padding + the minimum text column.
        assert_eq!(visual_width(LOGO) + 2 * H_PAD + RIGHT_COL_MIN, 143);
        assert_eq!(count_lines(LOGO) + 2 * V_PAD, 39);
        assert_eq!(visual_width(LOGO_SMALL) + 2 * H_PAD + RIGHT_COL_MIN, 113);
        assert_eq!(count_lines(LOGO_SMALL) + 2 * V_PAD, 26);
        // Stacked small-logo gate = logo extent + padding only.
        assert_eq!(visual_width(LOGO_SMALL) + 2 * H_PAD, 54);
    }

    #[test]
    fn shine_opacity_stays_in_unit_range() {
        let mut secs = 0.0;
        while secs < 10.0 {
            for i in 0..=20 {
                let diag = i as f32 / 20.0;
                let op = shine_opacity(diag, secs);
                assert!(
                    (0.0..=1.0).contains(&op),
                    "opacity {op} out of range at diag {diag}, secs {secs}"
                );
            }
            secs += 0.13;
        }
    }

    #[test]
    fn shine_band_sweeps_across() {
        // The brightest point along the diagonal advances left → right as the
        // sweep progresses through its active phase.
        let brightest = |secs: f32| -> f32 {
            (0..=100)
                .map(|i| i as f32 / 100.0)
                .max_by(|a, b| {
                    shine_opacity(*a, secs)
                        .partial_cmp(&shine_opacity(*b, secs))
                        .unwrap()
                })
                .unwrap()
        };
        let early = brightest(0.1);
        let mid = brightest(0.4);
        let late = brightest(0.7);
        assert!(early < mid, "early {early} should precede mid {mid}");
        assert!(mid < late, "mid {mid} should precede late {late}");
    }

    #[test]
    fn shine_rests_dim_between_sweeps() {
        // During the rest phase the band is parked off-screen, so an interior
        // glyph falls back to at most the gentle pulse — never full bright.
        let op = shine_opacity(0.5, 6.0); // secs % 4.0 = 2.0 → past SWEEP_FRAC, in the rest phase
        assert!(op < 0.2, "resting opacity {op} should stay dim");
    }

    #[test]
    fn loader_uses_builtins_when_directory_has_no_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let assets = load_logo_assets_from_dir(dir.path());

        assert_eq!(assets.art(LogoSize::Big), LOGO);
        assert_eq!(assets.art(LogoSize::Small), LOGO_SMALL);
    }

    #[test]
    fn loader_overrides_each_slot_independently() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("big.txt"), "BIG\nART").unwrap();
        let assets = load_logo_assets_from_dir(dir.path());

        assert_eq!(assets.art(LogoSize::Big), "BIG\nART");
        assert_eq!(assets.art(LogoSize::Small), LOGO_SMALL);
    }

    #[test]
    fn loader_accepts_both_valid_slots() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("big.txt"), "BIG\nART").unwrap();
        std::fs::write(dir.path().join("small.txt"), "SMALL").unwrap();
        let assets = load_logo_assets_from_dir(dir.path());

        assert_eq!(assets.art(LogoSize::Big), "BIG\nART");
        assert_eq!(assets.art(LogoSize::Small), "SMALL");
    }

    #[test]
    fn loader_falls_back_for_empty_and_invalid_utf8_slots() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("big.txt"), b"\n\n").unwrap();
        std::fs::write(dir.path().join("small.txt"), [0xff, 0xfe]).unwrap();
        let assets = load_logo_assets_from_dir(dir.path());

        assert_eq!(assets.art(LogoSize::Big), LOGO);
        assert_eq!(assets.art(LogoSize::Small), LOGO_SMALL);
    }

    #[test]
    fn loader_rejects_whitespace_and_terminal_control_slots() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("big.txt"), " \n  \n").unwrap();
        std::fs::write(dir.path().join("small.txt"), "G\tROW\u{1b}[31m").unwrap();
        let assets = load_logo_assets_from_dir(dir.path());

        assert_eq!(assets.art(LogoSize::Big), LOGO);
        assert_eq!(assets.art(LogoSize::Small), LOGO_SMALL);
    }

    #[test]
    fn loader_falls_back_for_oversized_slots() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("big.txt"),
            vec![b'X'; MAX_LOGO_BYTES as usize + 1],
        )
        .unwrap();
        std::fs::write(dir.path().join("small.txt"), "SMALL").unwrap();
        let assets = load_logo_assets_from_dir(dir.path());

        assert_eq!(assets.art(LogoSize::Big), LOGO);
        assert_eq!(assets.art(LogoSize::Small), "SMALL");
    }

    #[test]
    fn loader_normalizes_crlf_and_boundary_blank_lines() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("big.txt"), "\r\nA\r\n\r\nＢ\r\n").unwrap();
        let assets = load_logo_assets_from_dir(dir.path());
        let art = assets.art(LogoSize::Big);

        assert_eq!(art, "A\n\nＢ");
        assert!(!art.contains('\r'));
        assert_eq!(visual_width(art), 2);
        assert_eq!(count_lines(art), 3);
    }
}
