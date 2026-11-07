//! Integration tests for crash-handler.
//!
//! These tests verify that installing the crash handler does not interfere
//! with normal program operation (tokio runtime, signal handling, I/O),
//! and that it correctly captures crash data when a fatal signal fires.
//!
//! Tests that send fatal signals use subprocess isolation: the test process
//! re-executes itself with an env var that selects the crash scenario, so
//! the parent can verify outcomes without dying.

#![cfg(unix)]

use std::path::Path;
use std::process::Command;

fn only_capture(dir: &Path) -> std::path::PathBuf {
    let paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("capture-")
        })
        .collect();
    assert_eq!(paths.len(), 1, "one unique raw per installation");
    paths.into_iter().next().unwrap()
}

/// Re-invoke the current test binary as a subprocess with the given scenario.
/// Returns (exit status, stdout, stderr).
fn run_scenario(scenario: &str, crash_dir: &Path) -> (std::process::ExitStatus, String, String) {
    let exe = std::env::current_exe().expect("current_exe");
    let output = Command::new(exe)
        .env("CRASH_TEST_SCENARIO", scenario)
        .env("CRASH_TEST_DIR", crash_dir.as_os_str())
        .arg("--ignored")
        .arg("--exact")
        .arg("--nocapture")
        .arg("subprocess_entry")
        .output()
        .expect("failed to spawn subprocess");
    (
        output.status,
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

// ── Subprocess entry point ──────────────────────────────────────────────

/// This test is `#[ignore]`d so it only runs when invoked as a subprocess
/// by the parent test via `run_scenario`. The `CRASH_TEST_SCENARIO` env
/// var selects which scenario to execute.
#[test]
#[ignore]
fn subprocess_entry() {
    let scenario = match std::env::var("CRASH_TEST_SCENARIO") {
        Ok(s) => s,
        Err(_) => return, // not a subprocess invocation
    };
    let crash_dir = std::env::var("CRASH_TEST_DIR").expect("CRASH_TEST_DIR");
    let crash_dir = std::path::PathBuf::from(crash_dir);

    // Install the crash handler before anything else.
    let config = crash_handler::CrashHandlerConfig {
        app_version: "0.0.0-test".to_string(),
        crash_dir,
    };
    crash_handler::install(config);

    match scenario.as_str() {
        // Scenario 1: install handler, run tokio runtime with concurrent work, exit cleanly.
        "tokio_normal" => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async {
                // Spawn several concurrent tasks to stress the runtime.
                let mut handles = Vec::new();
                for i in 0..20 {
                    handles.push(tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                        i * i
                    }));
                }
                let mut sum = 0u64;
                for h in handles {
                    sum += h.await.unwrap();
                }
                // Also test signal infrastructure coexistence.
                // Register a tokio SIGTERM handler (same as the pager does).
                #[cfg(unix)]
                {
                    use tokio::signal::unix::{SignalKind, signal};
                    let _term = signal(SignalKind::terminate())
                        .expect("tokio SIGTERM handler should work alongside crash handler");
                }
                eprintln!("tokio_normal: sum={sum}, all tasks completed");
            });
        }

        // Scenario 2: install handler, do sync file I/O and computation, exit cleanly.
        "sync_normal" => {
            let tmp = tempfile::tempdir().expect("tempdir");
            for i in 0..50 {
                let path = tmp.path().join(format!("file-{i}.txt"));
                std::fs::write(&path, format!("contents {i}")).expect("write");
                let data = std::fs::read_to_string(&path).expect("read");
                assert!(data.contains(&format!("{i}")));
            }
            eprintln!("sync_normal: 50 files written and read back");
        }

        // Scenario 3: install handler, send ourselves SIGBUS, verify crash file written.
        "sigbus" => {
            // Give the handler a moment to be fully installed, then crash.
            unsafe { libc::raise(libc::SIGBUS) };
        }

        // Scenario 4: install handler, send ourselves SIGSEGV.
        "sigsegv" => {
            unsafe { libc::raise(libc::SIGSEGV) };
        }

        // Scenario 6: install handler, abort. This is the path every Rust
        // panic takes in release builds (panic = "abort" → SIGABRT).
        "sigabrt" => {
            std::process::abort();
        }

        // Scenario 5: tokio runtime + signal coexistence, then clean shutdown.
        "tokio_signals" => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async {
                use tokio::signal::unix::{SignalKind, signal};
                let mut usr1 = signal(SignalKind::user_defined1()).expect("SIGUSR1 handler");

                // Send ourselves SIGUSR1 and verify tokio receives it
                // (proves our SIGBUS/SIGSEGV handler doesn't clobber other signals).
                unsafe { libc::raise(libc::SIGUSR1) };
                tokio::time::timeout(std::time::Duration::from_secs(2), usr1.recv())
                    .await
                    .expect("SIGUSR1 should arrive within 2s");

                eprintln!("tokio_signals: SIGUSR1 received, signal coexistence OK");
            });
        }

        // Scenario: print this process's load address of the known
        // non-inline probe function and exit cleanly. The parent compares
        // it with addresses from other spawned processes to observe
        // address-space slides (ASLR).
        "symbol_probe" => {
            use std::io::Write as _;
            println!("PROBE_ADDR={:#x}", symbolication_probe_address());
            let _ = std::io::stdout().flush();
        }

        // Scenario: print the probe address, then fault with SIGSEGV from
        // inside the probe function. `frames[0]` is the faulting PC
        // (written before frame walking), so the capture contains this
        // process's real address inside that function.
        "capture_in_probe_function" => {
            use std::io::Write as _;
            println!("PROBE_ADDR={:#x}", symbolication_probe_address());
            let _ = std::io::stdout().flush();
            symbolication_probe(true);
            unreachable!("volatile null-page store must fault");
        }

        other => {
            eprintln!("unknown scenario: {other}");
            std::process::exit(99);
        }
    }
}

// ── Parent test cases ───────────────────────────────────────────────────

#[test]
fn handler_does_not_interfere_with_tokio_runtime() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (status, _stdout, stderr) = run_scenario("tokio_normal", tmp.path());
    assert!(
        status.success(),
        "tokio_normal should exit 0, got {status:?}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("all tasks completed"),
        "should see completion message\nstderr: {stderr}"
    );
    // A clean installation leaves its own empty capture for claimed cleanup.
    let path = only_capture(tmp.path());
    assert_eq!(std::fs::metadata(path).unwrap().len(), 0);
}

#[test]
fn handler_does_not_interfere_with_sync_io() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (status, _stdout, stderr) = run_scenario("sync_normal", tmp.path());
    assert!(
        status.success(),
        "sync_normal should exit 0, got {status:?}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("50 files written"),
        "should see completion message\nstderr: {stderr}"
    );
}

#[test]
fn handler_does_not_clobber_other_signal_handlers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (status, _stdout, stderr) = run_scenario("tokio_signals", tmp.path());
    assert!(
        status.success(),
        "tokio_signals should exit 0, got {status:?}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("signal coexistence OK"),
        "SIGUSR1 should be delivered through tokio\nstderr: {stderr}"
    );
}

#[test]
fn sigbus_produces_valid_crash_blob() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (status, _stdout, _stderr) = run_scenario("sigbus", tmp.path());

    // Process should have been killed by a signal.
    // We expect SIGBUS, but the frame-pointer walker may hit unmapped memory
    // and cause a secondary SIGSEGV (SA_RESETHAND ensures it terminates).
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let sig = status.signal();
        assert!(
            sig == Some(libc::SIGBUS) || sig == Some(libc::SIGSEGV),
            "process should be killed by SIGBUS or SIGSEGV, got signal={sig:?} status={status:?}"
        );
    }

    // The crash file should be parseable.
    let crash_file = only_capture(tmp.path());
    assert!(crash_file.exists(), "crash file should exist after SIGBUS");
    let data = std::fs::read(&crash_file).expect("read crash file");
    assert!(
        data.len() > 4,
        "crash file should have data, got {} bytes",
        data.len()
    );

    let blob = crash_handler::format::CrashBlob::parse(&data).expect("crash blob should parse");

    // On macOS SIGBUS=10, on Linux SIGBUS=7, SIGSEGV=11 on both.
    // The frame-pointer walker may cause a secondary SIGSEGV.
    assert!(
        blob.signal == 7 || blob.signal == 10 || blob.signal == 11,
        "signal should be SIGBUS or SIGSEGV, got {}",
        blob.signal
    );
    assert_eq!(blob.app_version, "0.0.0-test");
    assert!(blob.pid > 0, "PID should be nonzero");
    assert!(blob.timestamp > 0, "timestamp should be nonzero");

    // check_previous_crash should produce a report.
    let report = crash_handler::check_previous_crash(tmp.path())
        .expect("recovery should not fail")
        .expect("should produce a crash report");
    assert!(report.signal_name.contains("SIGBUS"));
    assert_eq!(report.app_version, "0.0.0-test");
    assert!(report.report_path.exists(), "report file should be written");

    // Crash blob should be consumed (deleted).
    assert!(
        !crash_file.exists(),
        "crash file should be deleted after processing"
    );
}

#[test]
fn sigsegv_produces_valid_crash_blob() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (status, _stdout, _stderr) = run_scenario("sigsegv", tmp.path());

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let sig = status.signal();
        assert_eq!(
            sig,
            Some(libc::SIGSEGV),
            "process should be killed by SIGSEGV, got signal={sig:?} status={status:?}"
        );
    }

    let crash_file = only_capture(tmp.path());
    assert!(crash_file.exists(), "crash file should exist after SIGSEGV");
    let data = std::fs::read(&crash_file).expect("read crash file");
    let blob = crash_handler::format::CrashBlob::parse(&data).expect("crash blob should parse");
    assert_eq!(blob.signal, 11, "signal should be SIGSEGV (11)");
    assert_eq!(blob.app_version, "0.0.0-test");
}

#[test]
fn sigabrt_produces_valid_crash_blob() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (status, _stdout, _stderr) = run_scenario("sigabrt", tmp.path());

    // The handler must re-raise with default disposition so the process
    // still dies with SIGABRT semantics. The frame-pointer walker may hit
    // unmapped memory and cause a secondary SIGSEGV (as in the SIGBUS test).
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let sig = status.signal();
        assert!(
            sig == Some(libc::SIGABRT) || sig == Some(libc::SIGSEGV),
            "process should be killed by SIGABRT (or a secondary SIGSEGV), got signal={sig:?} status={status:?}"
        );
    }

    let crash_file = only_capture(tmp.path());
    assert!(crash_file.exists(), "crash file should exist after abort()");
    let data = std::fs::read(&crash_file).expect("read crash file");
    let blob = crash_handler::format::CrashBlob::parse(&data).expect("crash blob should parse");
    assert_eq!(
        blob.signal, 6,
        "signal should be SIGABRT (6), got {}",
        blob.signal
    );
    assert_eq!(blob.app_version, "0.0.0-test");
    assert!(blob.pid > 0, "PID should be nonzero");
    assert!(blob.timestamp > 0, "timestamp should be nonzero");

    // check_previous_crash should produce a SIGABRT-labelled report.
    let report = crash_handler::check_previous_crash(tmp.path())
        .expect("recovery should not fail")
        .expect("should produce a crash report");
    assert!(
        report.signal_name.contains("SIGABRT"),
        "report should name SIGABRT, got {}",
        report.signal_name
    );
    assert_eq!(report.app_version, "0.0.0-test");
    assert!(report.report_path.exists(), "report file should be written");
    assert!(
        !crash_file.exists(),
        "crash file should be deleted after processing"
    );
}

#[test]
fn clean_exit_does_not_produce_crash_report() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Run both scenarios and verify no crash artifacts.
    for scenario in &["tokio_normal", "sync_normal", "tokio_signals"] {
        let (status, _stdout, stderr) = run_scenario(scenario, tmp.path());
        assert!(status.success(), "{scenario} failed: {stderr}");
    }
    // check_previous_crash should return None.
    let report = crash_handler::check_previous_crash(tmp.path())
        .expect("recovery should not fail on clean exits");
    assert!(report.is_none(), "no crash report after clean exits");
}

// ── PB-06: cross-process address provenance ─────────────────────────────

/// Known non-inline function serving as the address probe of every process
/// of this test binary: separately spawned PIE processes normally load the
/// same functions at different addresses, which is exactly why captured
/// raw addresses must not be resolved against another process's map. The
/// `crash` path faults with SIGSEGV from inside this function so the
/// handler records one of its real instruction addresses as frames[0].
#[inline(never)]
fn symbolication_probe(crash: bool) {
    if crash {
        // Volatile store through the null page: reliably SIGSEGV with the
        // faulting PC inside this function. Crash scenario only.
        unsafe { std::ptr::null_mut::<u8>().write_volatile(0) };
    }
    std::hint::black_box(());
}

fn symbolication_probe_address() -> usize {
    std::hint::black_box(symbolication_probe as fn(bool) as usize)
}

/// Parse the `PROBE_ADDR=` line a scenario child prints to stdout.
fn parse_probe_address(stdout: &str) -> usize {
    let line = stdout
        .lines()
        .find(|l| l.starts_with("PROBE_ADDR="))
        .expect("scenario child printed PROBE_ADDR");
    usize::from_str_radix(
        line.strip_prefix("PROBE_ADDR=")
            .unwrap()
            .strip_prefix("0x")
            .unwrap(),
        16,
    )
    .expect("probe address is hex")
}

/// Blob bytes laid out like the lib.rs fixture, parameterized by frames so
/// tests can plant specific raw addresses. Built from the public format
/// constants; this is an external integration test.
fn blob_bytes_with_frames(frames: &[usize]) -> Vec<u8> {
    use crash_handler::format::{HEADER_SIZE, MAGIC, VERSION, VERSION_STRING_LEN};
    let mut b = Vec::new();
    b.extend_from_slice(&MAGIC);
    b.push(VERSION);
    b.push(11); // SIGSEGV
    b.extend_from_slice(&1i32.to_le_bytes()); // SEGV_MAPERR
    b.extend_from_slice(&0u64.to_le_bytes()); // si_addr
    b.extend_from_slice(&4242u32.to_le_bytes()); // pid
    b.extend_from_slice(&1_700_000_000u64.to_le_bytes());
    b.extend_from_slice(&(frames.len() as u16).to_le_bytes());
    let mut version_field = [0u8; VERSION_STRING_LEN];
    version_field[..b"0.0.0-test".len()].copy_from_slice(b"0.0.0-test");
    b.extend_from_slice(&version_field);
    for frame in frames {
        b.extend_from_slice(&(*frame as u64).to_le_bytes());
    }
    debug_assert_eq!(b.len(), HEADER_SIZE + frames.len() * 8);
    b
}

/// The recovering process's own function address planted as a captured raw
/// ip is exactly the overlap the previous resolver turned into a false
/// "the crash happened in this current function" attribution.
#[test]
fn recovery_does_not_attribute_current_process_addresses() {
    let ip = symbolication_probe_address();
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        tmp.path().join("last-crash.bin"),
        blob_bytes_with_frames(&[ip]),
    )
    .expect("seed raw blob");

    let report = crash_handler::check_previous_crash(tmp.path())
        .expect("valid blob must recover")
        .expect("and produce a report");

    assert_eq!(report.backtrace.len(), 1);
    assert_eq!(report.backtrace[0].ip, ip, "raw ip preserved verbatim");
    assert!(
        report.backtrace[0].symbol_name.is_none(),
        "unverifiable address must not be resolved against the current map, got {:?}",
        report.backtrace[0].symbol_name
    );
    assert!(report.backtrace[0].filename.is_none());
    assert!(report.backtrace[0].lineno.is_none());

    let text = std::fs::read_to_string(&report.report_path).expect("read report");
    assert!(text.contains("previous process"));
    assert!(text.contains("symbolication is unavailable"));
    assert!(text.contains(&format!("{ip:#018x}")));
    assert!(
        !text.contains("symbolication_probe"),
        "report must not attribute the raw address to the current process:\n{text}"
    );
}

/// A capture from one spawned process recovered in another process's
/// address map: every raw address survives verbatim, nothing is
/// attributed, and the report states the missing provenance.
#[test]
fn cross_process_capture_preserves_raw_addresses_without_attribution() {
    // Process 1: separately spawned PIE process that prints its own probe
    // address and crashes from inside the probe function.
    let crash_tmp = tempfile::tempdir().expect("tempdir");
    let (status, crash_stdout, _stderr) =
        run_scenario("capture_in_probe_function", crash_tmp.path());
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            status.signal(),
            Some(libc::SIGSEGV),
            "crash scenario dies with SIGSEGV, got {status:?}"
        );
    }

    // Process 2: yet another separately spawned process of the same binary.
    let probe_tmp = tempfile::tempdir().expect("tempdir");
    let (_status, probe_stdout, _stderr) = run_scenario("symbol_probe", probe_tmp.path());

    let crash_child_probe = parse_probe_address(&crash_stdout);
    let probe_child_probe = parse_probe_address(&probe_stdout);
    if crash_child_probe != probe_child_probe {
        eprintln!(
            "cross-slide premise observed: probe at {crash_child_probe:#x} in the capture process vs {probe_child_probe:#x} in the second spawned process"
        );
    } else {
        // State the non-coverage explicitly instead of weakening or faking
        // the slide difference; raw fidelity below does not depend on it.
        eprintln!(
            "ASLR slide difference not observed between spawned processes ({crash_child_probe:#x}); cross-slide difference not covered in this environment"
        );
    }

    let crash_file = only_capture(crash_tmp.path());
    let data = std::fs::read(&crash_file).expect("read capture");
    let blob = crash_handler::format::CrashBlob::parse(&data).expect("capture parses");
    assert!(
        !blob.frames.is_empty(),
        "handler records the faulting pc as frames[0] before frame walking"
    );

    // Recovery in yet another address map (this test process).
    let report = crash_handler::check_previous_crash(crash_tmp.path())
        .expect("capture recovery succeeds")
        .expect("and produce a report");
    assert_eq!(report.backtrace.len(), blob.frames.len());
    for (frame, ip) in report.backtrace.iter().zip(&blob.frames) {
        assert_eq!(
            frame.ip, *ip,
            "raw addresses preserved verbatim across processes"
        );
        assert!(frame.symbol_name.is_none(), "no cross-process attribution");
        assert!(frame.filename.is_none());
        assert!(frame.lineno.is_none());
    }

    let text = std::fs::read_to_string(&report.report_path).expect("read report");
    assert!(text.contains("previous process"));
    assert!(text.contains("symbolication is unavailable"));
    for ip in &blob.frames {
        assert!(text.contains(&format!("{ip:#018x}")));
    }
    assert!(
        !text.contains("symbolication_probe"),
        "no attribution to any process's probe function:\n{text}"
    );
}
