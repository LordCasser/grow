//! A real rail click seeks beyond the recent body page, then returns live.
#[allow(unused_imports)]
use super::common::*;
use shell::session::storage::{JsonlStorageAdapter, StorageAdapter};
use shell::session::testkit::synth::{self, SessionSpec};
use unicode_width::UnicodeWidthStr;

const HISTORY_TURNS: usize = 300;
const RAIL_ROWS: u16 = 100;
const ARTIFACT_DIR: &str = "/tmp/grow-repair-session-resume-presentation";
const CAST_NAME: &str = "timeline-direct-seek.cast";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run pty_e2e_persistence with --ignored --test-threads=1"]
async fn timeline_history_direct_seek_returns_live() {
    let content = ContentController::start()
        .await
        .expect("start mock provider");
    content.seed_llm_config().expect("seed mock provider");
    seed_ui_config(&content, "show_timeline = true\nvim_mode = true");
    let project = tempfile::tempdir().expect("create project");
    git2::Repository::init(project.path()).expect("init project git");
    let project_cwd = dunce::canonicalize(project.path()).expect("canonical project cwd");
    let grow_home = content.home().join(".grow");
    let spec = SessionSpec {
        turns: HISTORY_TURNS,
        acu_per_turn: 1,
        catalog_commands: 1,
        catalog_desc_len: 16,
        agent_chunks_per_turn: 1,
        agent_chunk_len: 24,
        rewind_points: 0,
        files_per_rewind: 0,
        file_content_len: 0,
    };
    let (info, directory) =
        synth::replay::prepare_legacy_session(&grow_home, &project_cwd, &spec).await;
    let route = sampling_types::model_image_input_key_from_parts(
        "mock",
        &sampling_types::ApiBackend::ChatCompletions,
        &content.url(),
        &Default::default(),
    );
    synth::write_admitted_history(
        &info,
        &directory,
        &spec,
        "mock/mock",
        0,
        Some(("mock", &route)),
    );
    let storage = JsonlStorageAdapter::with_root(grow_home.clone());
    storage
        .update_current_model_and_agent(
            &info,
            &shell::agent::models::ModelId::new("mock/mock"),
            None,
            None,
        )
        .await
        .expect("select mock model for fixture");
    // Release the fixture writer lease before the real process acquires it.
    drop(storage);
    let binary = pager_binary().expect("resolve pager binary");
    let mut session = PtyHarness::spawn_with_content_in_dir(
        &binary,
        RAIL_ROWS,
        DEFAULT_COLS,
        &content,
        &["--continue", "--no-leader", "--trust"],
        Some(&project_cwd),
    )
    .expect("spawn resumed history");
    let latest = format!("GROW_SEQ_{HISTORY_TURNS:06}");
    let ready = session.wait_until_stable(
        "recent marker and global timeline rail",
        RESUME_TIMEOUT,
        Duration::from_millis(400),
        |h| {
            h.contains_text(&latest)
                && h.contains_text("grow · Mock")
                && !h.contains_text("Loading session")
                && first_tick(&h.screen_contents()).is_some()
        },
    );
    if let Err(error) = ready {
        fail(&session, &format!("recent history did not load: {error}"));
    }
    let opened = session.screen_contents();
    let (tick_row, tick_col, tick_count) =
        first_tick(&opened).unwrap_or_else(|| fail(&session, "right-hand rail was not present"));
    // compute_rail pins a bottom window of (scrollback height - 2) ticks.
    // This fixture has two visible rows per turn, so Latest's 128-row
    // budget admits at most 64 prompts. A tall terminal makes the first
    // rail tick precede that body window, rather than merely its viewport.
    expect_pty(
        &session,
        tick_count > 64,
        "rail cannot reach outside the recent 128-update page",
    );
    let ordinal = HISTORY_TURNS - tick_count;
    let prompt = format!("user prompt for turn {ordinal}");
    let target = format!("GROW_SEQ_{:06}", ordinal + 1);
    expect_pty(
        &session,
        !opened.contains("GROW_SEQ_000001")
            && !session.full_text().contains("GROW_SEQ_000001")
            && !opened.contains(&target)
            && !session.full_text().contains(&target)
            && !opened.contains(&prompt)
            && !session.full_text().contains(&prompt),
        "early or target body was already displayed before the rail click",
    );
    let requests_before = content.request_count();
    // The admitted fixture need not contain any model settlements. Comparing
    // the complete verified collection still detects billing caused by browsing.
    let ledger_before = usage_ledger(&grow_home, &info, &session).await;
    let click = format!(
        "{}{}",
        sgr_mouse(0, tick_row, tick_col, 'M'),
        sgr_mouse(0, tick_row, tick_col, 'm'),
    );
    if let Err(error) = session.inject_keys(click.as_bytes()) {
        fail(&session, &format!("rail click failed: {error}"));
    }
    let sought = session.wait_until_stable(
        "unloaded first-tick body anchored near the top",
        Duration::from_secs(20),
        Duration::from_millis(400),
        |h| {
            let screen = h.screen_contents();
            let top = tick_row.saturating_sub(1) as usize;
            let lines: Vec<_> = screen.lines().collect();
            lines
                .iter()
                .skip(top)
                .take(5)
                .any(|line| line.contains(&prompt))
                && lines
                    .iter()
                    .skip(top)
                    .take(12)
                    .any(|line| line.contains(&target))
                && !screen.contains(&latest)
        },
    );
    if let Err(error) = sought {
        fail(
            &session,
            &format!("rail ordinal {ordinal} did not reveal its body: {error}"),
        );
    }
    expect_pty(
        &session,
        content.request_count() == requests_before
            && usage_ledger(&grow_home, &info, &session).await == ledger_before,
        "direct history seek changed model requests or typed usage",
    );

    // The real rail click focuses scrollback (mouse.rs); G is its configured
    // vim GotoBottom binding, so no guessed Tab toggle is needed here.
    if let Err(error) = session.inject_keys(b"G") {
        fail(&session, &format!("return-live key failed: {error}"));
    }
    let live = session.wait_until_stable(
        "live tail after direct seek",
        Duration::from_secs(20),
        Duration::from_millis(400),
        |h| h.contains_text(&latest) && !h.contains_text(&target) && !h.contains_text(&prompt),
    );
    if let Err(error) = live {
        fail(&session, &format!("return to live failed: {error}"));
    }
    expect_pty(
        &session,
        content.request_count() == requests_before
            && usage_ledger(&grow_home, &info, &session).await == ledger_before,
        "returning live changed model requests or typed usage",
    );
    if let Err(error) = session.inject_keys(b"\x04\x04") {
        fail(&session, &format!("quit injection failed: {error}"));
    }
    match session.wait_for_exit_and_drain(Duration::from_secs(20), Duration::from_secs(1)) {
        Ok(0) => {}
        exit => fail(&session, &format!("quit did not exit cleanly: {exit:?}")),
    }
}

// Locate the two-column rail from actual rendered glyphs, preserving screen
// row numbers and terminal display columns even when body text is wide.
fn first_tick(screen: &str) -> Option<(u16, u16, usize)> {
    let lines: Vec<_> = screen.lines().collect();
    for (up, line) in lines.iter().enumerate() {
        let trimmed = line.trim_end();
        let Some(prefix) = trimmed
            .strip_suffix('▴')
            .or_else(|| trimmed.strip_suffix('▲'))
        else {
            continue;
        };
        let col = UnicodeWidthStr::width(prefix);
        if col < DEFAULT_COLS as usize - 2 {
            continue;
        }
        let mut ticks = 0;
        for line in lines.iter().skip(up + 1) {
            let trimmed = line.trim_end();
            if UnicodeWidthStr::width(trimmed) != col + 1 {
                break;
            }
            if trimmed.ends_with(['▾', '▼']) {
                return (ticks > 0).then_some(((up + 1) as u16, col as u16, ticks));
            }
            if !trimmed.ends_with(['─', '━', '═']) {
                break;
            }
            ticks += 1;
        }
    }
    None
}

#[derive(Debug, PartialEq, Eq)]
struct SettledAttempt {
    owner: chat_state::UsagePromptOwner,
    prompt_tokens: u64,
    completion_tokens: u64,
}

async fn usage_ledger(
    grow_home: &Path,
    info: &shell::session::info::Info,
    session: &PtyHarness,
) -> Vec<SettledAttempt> {
    let storage = JsonlStorageAdapter::with_root(grow_home.to_path_buf());
    let data = storage
        .load_session(info)
        .await
        .unwrap_or_else(|error| fail(session, &format!("load usage ledger: {error}")));
    data.timeline_events
        .iter()
        .filter_map(|event| {
            let chat_state::TimelineEventKind::Observation(observation) = &event.kind else {
                return None;
            };
            if observation.scope != "sampling_usage" || observation.name != "attempt_settled" {
                return None;
            }
            let payload = observation
                .data
                .as_ref()
                .unwrap_or_else(|| fail(session, "settled attempt has no payload"));
            let owner = serde_json::from_value(payload["prompt_owner"].clone())
                .unwrap_or_else(|error| fail(session, &format!("typed prompt owner: {error}")));
            let tokens = |key: &str| {
                payload["usage"][key]
                    .as_u64()
                    .unwrap_or_else(|| fail(session, &format!("settled attempt missing {key}")))
            };
            Some(SettledAttempt {
                owner,
                prompt_tokens: tokens("prompt_tokens"),
                completion_tokens: tokens("completion_tokens"),
            })
        })
        .collect()
}

fn expect_pty(session: &PtyHarness, condition: bool, message: &str) {
    if !condition {
        fail(session, message);
    }
}

fn fail(session: &PtyHarness, message: &str) -> ! {
    let path = Path::new(ARTIFACT_DIR).join(CAST_NAME);
    let write_error = std::fs::create_dir_all(ARTIFACT_DIR)
        .map_err(|error| error.to_string())
        .and_then(|()| session.write_cast(&path).map_err(|error| error.to_string()))
        .err();
    panic!(
        "{message}\ncast: {} (write error: {write_error:?})\nscreen:\n{}\nfull:\n{}",
        path.display(),
        session.screen_contents(),
        session.full_text(),
    );
}
