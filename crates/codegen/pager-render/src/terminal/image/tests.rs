use super::*;

#[test]
fn protocol_matrix_matches_supported_terminals() {
    for (brand, expected) in [
        (TerminalName::Kitty, GraphicsProtocol::Kitty),
        (TerminalName::Ghostty, GraphicsProtocol::Kitty),
        (TerminalName::WezTerm, GraphicsProtocol::Kitty),
        (TerminalName::WarpTerminal, GraphicsProtocol::Kitty),
        (TerminalName::Iterm2, GraphicsProtocol::None),
        (TerminalName::Unknown, GraphicsProtocol::None),
    ] {
        assert_eq!(protocol_for_brand(brand, false), expected);
        assert_eq!(protocol_for_brand(brand, true), GraphicsProtocol::None);
    }
}

#[test]
fn scrollback_overlay_excludes_warp() {
    assert!(scrollback_inline_overlay_active_for_brand(
        GraphicsProtocol::Kitty,
        TerminalName::Kitty,
    ));
    assert!(!scrollback_inline_overlay_active_for_brand(
        GraphicsProtocol::Kitty,
        TerminalName::WarpTerminal,
    ));
}

#[test]
#[serial_test::serial]
fn force_off_overrides_capability() {
    let _guard = set_protocol_for_test(GraphicsProtocol::Kitty);
    set_inline_overlay_force_off(false);
    assert!(scrollback_inline_overlay_active());
    set_inline_overlay_force_off(true);
    assert!(!scrollback_inline_overlay_active());
    set_inline_overlay_force_off(false);
}

#[test]
fn kitty_escape_chunks_and_preserves_cursor() {
    let small = render_kitty_image(&[0u8; 10], KittyImageFormat::Png, 40, 20);
    assert!(small.contains("a=T"));
    assert!(small.contains("f=100"));
    assert!(small.contains("q=2"));
    assert!(small.contains("C=1"));
    assert!(small.contains("c=40"));
    assert!(small.contains("r=20"));
    assert!(small.contains("m=0"));
    let large = render_kitty_image(&vec![0u8; 5000], KittyImageFormat::Png, 40, 20);
    assert!(large.matches("\x1b_G").count() > 1);
}

#[test]
fn kitty_format_and_conversion_produce_png() {
    use image::{ImageBuffer, Rgb};

    let png = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    assert_eq!(kitty_format_from_bytes(&png), Some(KittyImageFormat::Png));
    let buffer: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(4, 3, Rgb([128, 64, 32]));
    let mut jpeg = Vec::new();
    buffer
        .write_to(
            &mut std::io::Cursor::new(&mut jpeg),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    assert_eq!(kitty_format_from_bytes(&jpeg), None);
    let converted = prepare_kitty_overlay_image_bytes(&jpeg).unwrap();
    assert_eq!(
        kitty_format_from_bytes(&converted),
        Some(KittyImageFormat::Png)
    );
}

#[test]
fn overlay_conversion_pixel_budget_checks_jpeg_headers_without_decoding() {
    let buffer = image::ImageBuffer::from_pixel(1, 1, image::Rgb([1u8, 2, 3]));
    let mut jpeg = Vec::new();
    buffer.write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg).unwrap();
    let sof = jpeg.windows(2).position(|marker| marker == [0xff, 0xc0]).unwrap();
    for (width, height, allowed) in [(4000u16, 4000u16, true), (4001, 4000, false), (65535, 65535, false)] {
        let mut header = jpeg.clone();
        header[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
        header[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
        let dimensions = tools::util::image_validate::validate_image_bytes_unrestricted(&header, false).unwrap();
        assert_eq!((dimensions.0, dimensions.1), (u32::from(width), u32::from(height)));
        assert_eq!(overlay_conversion_within_pixel_budget(&header), allowed);
        if !allowed {
            let original = header.clone();
            assert!(prepare_kitty_overlay_image_bytes(&header).is_none());
            assert_eq!(header, original);
        }
    }
    assert!(!overlay_conversion_within_pixel_budget(&[]));
    assert!(!overlay_conversion_within_pixel_budget(b"not an image"));
}

#[test]
fn overlay_conversion_budget_keeps_direct_png_bytes() {
    let png = b"\x89PNG\r\n\x1a\n";
    assert_eq!(prepare_kitty_overlay_image_bytes(png).unwrap(), png);
}

#[test]
fn sips_workspaces_have_independent_owned_lifetimes() {
    let first = sips_temp_directory().unwrap();
    let second = sips_temp_directory().unwrap();
    assert_ne!(first.path(), second.path());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for directory in [&first, &second] {
            assert_eq!(std::fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777, 0o700);
        }
    }
    let first_path = first.path().to_owned();
    drop(first);
    assert!(!first_path.exists());
    assert!(second.path().exists());
}

#[cfg(unix)]
#[test]
fn sips_workspaces_are_removed_after_process_success_and_failure() {
    for (script, succeeds) in [
        ("cp \"$4\" \"$6\"", true),
        ("cp \"$4\" \"$6\"; exit 7", false),
        ("exit 0", false),
        ("mkdir \"$6\"", false),
        (": > \"$6\"", false),
        ("ln -s \"$4\" \"$6\"", false),
    ] {
        let directory = sips_temp_directory().unwrap();
        let path = directory.path().to_owned();
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", script, "sips-test"]);
        let result = convert_via_sips_in(b"source bytes", directory, command);
        if succeeds { assert_eq!(result.unwrap(), b"source bytes"); }
        else { assert!(result.is_none()); }
        assert!(!path.exists(), "temporary files leaked after {script}");
    }
}

#[test]
fn sips_workspaces_are_removed_after_early_failure() {
    for block_source in [false, true] {
        let directory = sips_temp_directory().unwrap();
        let path = directory.path().to_owned();
        if block_source { std::fs::create_dir(path.join("source.dat")).unwrap(); }
        let command = std::process::Command::new(path.join("missing-executable"));
        assert!(convert_via_sips_in(b"source bytes", directory, command).is_none());
        assert!(!path.exists());
    }
}

#[test]
#[cfg(unix)]
fn sips_runner_preserves_exit_status() {
    for code in [0, 7] {
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", &format!("exit {code}")]);
        let result = run_sips_command(command, std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(result.code(), Some(code));
    }
}

#[test]
#[cfg(unix)]
fn sips_runner_stops_timed_out_and_orphaned_group_members() {
    use std::time::{Duration, Instant};
    for leader_exits in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("late-output");
        let script = if leader_exits {
            "(sleep 0.3; printf leaked > \"$1\") & exit 0"
        } else {
            "(sleep 0.3; printf leaked > \"$1\") & sleep 5"
        };
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", script, "sips-test"]).arg(&marker);
        let started = Instant::now();
        let result = run_sips_command(command, Duration::from_millis(50));
        if leader_exits { assert!(result.unwrap().success()); }
        else { assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut); }
        assert!(started.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(450));
        assert!(!marker.exists(), "owned descendant survived converter completion");
    }
}

#[test]
fn sips_output_reader_bounds_actual_consumption() {
    for size in [0, 8, 9, 128] {
        let mut reader = std::io::Cursor::new(vec![42; size]);
        let output = read_sips_output_bytes(&mut reader, 8);
        assert_eq!(reader.position(), size.min(9) as u64);
        if size == 8 { assert_eq!(output.unwrap(), vec![42; 8]); }
        else { assert!(output.is_none()); }
    }
    struct FailedRead;
    impl std::io::Read for FailedRead {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        }
    }
    assert!(read_sips_output_bytes(FailedRead, 8).is_none());
}

#[cfg(unix)]
#[test]
fn sips_oversized_output_is_rejected_and_removed() {
    let directory = sips_temp_directory().unwrap();
    let path = directory.path().to_owned();
    let file = std::fs::File::create(path.join("output.png")).unwrap();
    file.set_len(MAX_SIPS_OUTPUT_BYTES as u64 + 1).unwrap();
    drop(file);
    let mut command = std::process::Command::new("/bin/sh");
    command.args(["-c", "exit 0"]);
    assert!(convert_via_sips_in(b"input", directory, command).is_none());
    assert!(!path.exists());
}

#[test]
fn sips_output_growth_after_metadata_still_stops_at_budget() {
    use std::io::{Seek, Write};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("output.png");
    std::fs::write(&path, b"1234").unwrap();
    let mut reader = std::fs::File::open(&path).unwrap();
    assert_eq!(reader.metadata().unwrap().len(), 4);
    let mut writer = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    writer.write_all(&[7; 64]).unwrap();
    drop(writer);
    assert!(read_sips_output_bytes(&mut reader, 4).is_none());
    assert_eq!(reader.stream_position().unwrap(), 5);
}

#[cfg(unix)]
#[test]
fn sips_output_fifo_is_rejected_without_blocking() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("output.png");
    let status = std::process::Command::new("/usr/bin/mkfifo").arg(&path).status().unwrap();
    assert!(status.success());
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || { sender.send(read_sips_output(&path)).unwrap(); });
    assert!(receiver.recv_timeout(std::time::Duration::from_secs(2)).unwrap().is_none());
    worker.join().unwrap();
}

#[test]
fn sips_startup_error_reports_stage_and_kind() {
    let directory = tempfile::tempdir().unwrap();
    let command = std::process::Command::new(directory.path().join("missing-program"));
    let error = run_sips_command(command, std::time::Duration::from_secs(1)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(error.to_string().contains("sips process startup failed:"));
}

#[cfg(unix)]
#[test]
fn sips_pre_exec_permission_error_is_identified_as_startup() {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new("/bin/sh");
    command.args(["-c", "exit 0"]);
    // SAFETY: this injected hook only constructs an errno-backed error.
    unsafe {
        command.pre_exec(|| Err(std::io::Error::from_raw_os_error(libc::EPERM)));
    }
    let error = run_sips_command(command, std::time::Duration::from_secs(1)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(error.to_string().contains("sips process startup failed:"));
    assert!(error.to_string().contains(&std::io::Error::from_raw_os_error(libc::EPERM).to_string()));
}

#[test]
fn iterm_escape_preserves_requested_geometry() {
    let escape = render_iterm2_image(&[0u8; 10], 30, 15);
    assert!(escape.starts_with("\x1b]1337;File="));
    assert!(escape.contains("width=30cells"));
    assert!(escape.contains("height=15cells"));
    assert!(escape.contains("preserveAspectRatio=1"));
}

#[test]
fn low_level_overlay_separates_transmit_from_placement() {
    let png = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    let protocol = GraphicsProtocol::Kitty;
    let first =
        build_overlay_image_escapes_for_protocol(protocol, &png, 20, 10, 0, 0, true).unwrap();
    let subsequent =
        build_overlay_image_escapes_for_protocol(protocol, &png, 20, 10, 0, 0, false).unwrap();
    assert!(first.contains("a=t") && first.contains("a=p"));
    assert!(!first.contains("a=T"));
    assert!(subsequent.contains("a=p"));
    assert!(!subsequent.contains("a=t"));
}

#[test]
fn iterm_place_can_skip_inline_data() {
    let _guard = set_protocol_for_test(GraphicsProtocol::ITerm2);
    let area = ratatui::layout::Rect::new(0, 0, 40, 20);
    let escape = place_inline_image(&[0u8; 10], 100, 50, area, 20, 0, 2, false).unwrap();
    assert!(escape.starts_with("\x1b["));
    assert!(!escape.contains("1337"));
}

#[test]
fn placement_only_steady_state_removes_payload_cost() {
    let mut png = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    png.extend(std::iter::repeat_n(0u8, 200_000));
    let protocol = GraphicsProtocol::Kitty;
    let first =
        build_overlay_image_escapes_for_protocol(protocol, &png, 40, 20, 0, 0, true).unwrap();
    let subsequent =
        build_overlay_image_escapes_for_protocol(protocol, &png, 40, 20, 0, 0, false).unwrap();
    assert!(first.len() > 200_000);
    assert!(subsequent.len() < 200);
    assert!(!subsequent.contains("a=t"));
}
