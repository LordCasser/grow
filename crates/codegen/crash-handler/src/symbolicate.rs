//! Crash report assembly for captured raw evidence.
//!
//! Startup recovery runs in a new process whose address map has no
//! established relationship to the crashing process's map. Capture format
//! v1 records only absolute instruction pointers (plus pid/version), not
//! module identity or load addresses, so this module preserves the raw
//! addresses and never consults a symbolizer: resolving them against the
//! current map would attribute the crash to whatever now occupies those
//! addresses (ASLR re-slides every run, binaries change between versions).
//! Equal pid or version strings do not establish address-space provenance.

use crate::format::CrashBlob;

/// A captured backtrace frame.
///
/// `ip` is the raw instruction pointer recorded by the signal handler in
/// the crashing process. The symbol fields are `None` whenever no verified
/// symbolication exists for that address; recovery never fills them from
/// the recovering process's own address map.
#[derive(Debug, Clone)]
pub struct ResolvedFrame {
    pub ip: usize,
    pub symbol_name: Option<String>,
    pub filename: Option<String>,
    pub lineno: Option<u32>,
}

/// Preserve the captured instruction pointers of a crash blob verbatim.
///
/// Capture format v1 stores absolute addresses without module identity or
/// load-address mapping, so the recovering process cannot verify that a
/// captured address belongs to any of its own images. Symbolicating such
/// addresses against the current map would attribute the crash to whatever
/// now occupies that address; equal pid or version strings do not
/// establish address-space provenance either. No resolver is consulted:
/// every address is returned as-is with all symbol fields left `None`.
/// Offline symbolication remains possible from the report's raw addresses
/// only with the crashing binary and its load mapping, which format v1
/// does not record.
pub fn resolve_frames(blob: &CrashBlob) -> Vec<ResolvedFrame> {
    blob.frames
        .iter()
        .map(|&ip| ResolvedFrame {
            ip,
            symbol_name: None,
            filename: None,
            lineno: None,
        })
        .collect()
}

/// Format a crash report as human-readable text.
pub fn format_report(blob: &CrashBlob, frames: &[ResolvedFrame]) -> String {
    let mut out = String::with_capacity(4096);

    out.push_str("=== Grow Crash Report ===\n\n");

    out.push_str(&format!("Signal:  {}\n", signal_name(blob.signal)));
    out.push_str(&format!(
        "si_code: {} ({})\n",
        blob.si_code,
        si_code_name(blob.signal, blob.si_code)
    ));
    out.push_str(&format!("Address: {:#018x}\n", blob.si_addr));
    out.push_str(&format!("PID:     {}\n", blob.pid));
    out.push_str(&format!("Version: {}\n", blob.app_version));

    // Format timestamp as ISO 8601 (best-effort without chrono dependency).
    out.push_str(&format!("Time:    {} (unix)\n", blob.timestamp));

    // Raw v1 captures carry no module identity or load addresses, so the
    // addresses below were recorded by a previous process and cannot be
    // verified against this process's images. State the limitation before
    // the backtrace so the report is never read as symbolicated output.
    out.push_str(
        "\nNote: frames are raw addresses from a previous process; module\nidentity and load addresses were not captured, so automatic\nsymbolication is unavailable and no function attribution is made.\n",
    );

    out.push_str(&format!("\nBacktrace ({} frames):\n", frames.len()));
    for (i, frame) in frames.iter().enumerate() {
        let name = frame.symbol_name.as_deref().unwrap_or("<unknown>");
        out.push_str(&format!("  {:>3}: {:#018x} - {}\n", i, frame.ip, name));
        if let (Some(file), Some(line)) = (&frame.filename, frame.lineno) {
            out.push_str(&format!("           at {}:{}\n", file, line));
        }
    }

    out.push_str("\n=== End Report ===\n");
    out
}

pub fn signal_name(sig: u8) -> &'static str {
    match sig as i32 {
        4 => "SIGILL (Illegal instruction)",
        // SIGABRT is 6 on both macOS and Linux.
        // With panic = "abort", every Rust panic terminates via SIGABRT.
        6 => "SIGABRT (Abort)",
        // SIGBUS is 10 on macOS, 7 on Linux
        7 | 10 => "SIGBUS (Bus error)",
        11 => "SIGSEGV (Segmentation fault)",
        _ => "Unknown signal",
    }
}

fn si_code_name(sig: u8, code: i32) -> &'static str {
    // SIGABRT carries no fault-specific si_code (abort(3) raises it directly;
    // the kernel reports SI_USER/SI_TKILL-style origins instead).
    if sig == 6 {
        return "abort() - raised by the process (e.g. Rust panic with panic=abort)";
    }
    let is_bus = sig == 7 || sig == 10;
    if is_bus {
        match code {
            1 => "BUS_ADRALN - invalid address alignment",
            2 => "BUS_ADRERR - non-existent physical address",
            3 => "BUS_OBJERR - object-specific hardware error",
            _ => "unknown",
        }
    } else {
        match code {
            1 => "SEGV_MAPERR - address not mapped",
            2 => "SEGV_ACCERR - invalid permissions",
            _ => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A known non-inline function of the recovering (test) process. A
    /// pre-PB06 resolver attributed raw addresses pointing here to this
    /// very function; recovery must keep the address unresolved instead.
    #[inline(never)]
    fn current_process_function() {
        std::hint::black_box(());
    }

    fn current_process_function_address() -> usize {
        std::hint::black_box(current_process_function as fn() as usize)
    }

    #[test]
    fn resolve_frames_preserves_raw_addresses_without_attribution() {
        let blob = CrashBlob {
            signal: 11,
            si_code: 1,
            si_addr: 0,
            pid: 4242,
            timestamp: 1_700_000_000,
            frames: vec![
                current_process_function_address(),
                current_process_function_address() + 0x10,
            ],
            app_version: "0.0.0-tst".to_string(),
        };

        let frames = resolve_frames(&blob);

        assert_eq!(frames.len(), 2, "one output frame per captured ip");
        assert_eq!(
            frames[0].ip, blob.frames[0],
            "raw ip must be preserved verbatim"
        );
        assert_eq!(frames[1].ip, blob.frames[1]);
        for frame in &frames {
            assert!(
                frame.symbol_name.is_none(),
                "unverifiable address must not be attributed to the current map, got {:?}",
                frame.symbol_name
            );
            assert!(frame.filename.is_none());
            assert!(frame.lineno.is_none());
        }
    }

    #[test]
    fn format_report_states_symbolication_unavailable_for_raw_captures() {
        let ip = current_process_function_address();
        let blob = CrashBlob {
            signal: 11,
            si_code: 1,
            si_addr: 0,
            pid: 4242,
            timestamp: 1_700_000_000,
            frames: vec![ip],
            app_version: "0.0.0-tst".to_string(),
        };

        let report = format_report(&blob, &resolve_frames(&blob));

        assert!(
            report.contains("previous process"),
            "report must state the capture provenance:\n{report}"
        );
        assert!(report.contains("symbolication is unavailable"));
        assert!(report.contains("no function attribution is made"));
        assert!(
            report.contains(&format!("{ip:#018x}")),
            "raw ip must be preserved in the report"
        );
        assert!(
            !report.contains("current_process_function"),
            "report must not attribute the raw address to the current process:\n{report}"
        );
    }

    #[test]
    fn signal_names() {
        assert_eq!(signal_name(6), "SIGABRT (Abort)");
        assert_eq!(signal_name(10), "SIGBUS (Bus error)");
        assert_eq!(signal_name(7), "SIGBUS (Bus error)");
        assert_eq!(signal_name(11), "SIGSEGV (Segmentation fault)");
    }

    #[test]
    fn format_report_smoke() {
        let blob = CrashBlob {
            signal: 10,
            si_code: 2,
            si_addr: 0x7f8a_1234_0000,
            pid: 42,
            timestamp: 1_712_678_587,
            frames: vec![0xdead_beef],
            app_version: "0.1.169".to_string(),
        };
        let frames = vec![ResolvedFrame {
            ip: 0xdead_beef,
            symbol_name: Some("pager::main".to_string()),
            filename: Some("src/main.rs".to_string()),
            lineno: Some(42),
        }];
        let report = format_report(&blob, &frames);
        assert!(report.contains("SIGBUS"));
        assert!(report.contains("BUS_ADRERR"));
        assert!(report.contains("pager::main"));
        assert!(report.contains("src/main.rs:42"));
    }
}
