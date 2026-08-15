//! Human-readable rendering for `grow du`.

use super::DuReport;

/// Render the report as `Grow home:` / `Total:` plus aligned
/// `size  name` rows, largest first.
pub(super) fn format(report: &DuReport) -> String {
    let total = human_size(report.total_bytes);
    let width = report
        .entries
        .iter()
        .map(|entry| human_size(entry.bytes).len())
        .max()
        .unwrap_or(0)
        .max(total.len());
    let mut out = String::new();
    out.push_str("Grow home: ");
    out.push_str(&report.root.display().to_string());
    out.push('\n');
    out.push_str(&format!("Total: {:>width$}\n", total));
    if !report.entries.is_empty() {
        out.push('\n');
        for entry in &report.entries {
            out.push_str(&format!(
                "{:>width$}  {}\n",
                human_size(entry.bytes),
                entry.name
            ));
        }
    }
    out
}

/// Binary-unit size (`B`, `KiB`, … `EiB`) in the `du -h` convention.
pub(super) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
