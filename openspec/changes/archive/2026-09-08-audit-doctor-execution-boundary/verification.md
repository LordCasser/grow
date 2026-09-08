# Evidence

Source audit on main; no runtime doctor, tmux command, config mutation or build was executed.

- `slash/commands/doctor.rs` accepts report, fix listing and a known fix ID, rejecting extra tokens. It is session-scoped and registered in the normal command registry. The explicit fix forms are active functionality, not dead hidden switches.
- `doctor_cmd/mod.rs` handles standalone reports through collect_report/write_report. Fix application is a separate path: preview, stdin confirmation or --yes, then apply_fix. run_with_writer rejects fix commands. CLI main dispatches standalone doctor before runtime startup.
- TUI `dispatch/prompt.rs::dispatch_doctor` calls collect_live_doctor_report_for_terminal synchronously before matching Report versus fix requests. DoctorCommand::report_for_terminal calls collect_doctor_tui plus runtime findings and agent-definition discovery. Thus both /doctor and /doctor fix can perform filesystem/native/tmux work on the dispatcher thread.
- `diagnostics/probes/mod.rs::collect_tmux` uses sequential option-support, option-value and control-mode queries as needed. LiveTmuxProbe delegates to terminal/tmux_probe.rs. That runner has a 2-second main process deadline, a post-exit cleanup grace and process-group cleanup; bounded process lifetime does not make the synchronous UI wait asynchronous. No wall-clock reproduction was run and no aggregate latency guarantee is asserted.
- PlanDoctorFix and ApplyDoctorFix already use spawn_blocking, but the report is produced before these effects are returned. The TUI planning target captures agent id, session id, binding epoch and cwd; the confirmation uses an explicit local question. These guards should be preserved when moving report collection.

## Next change
background-doctor-report will move the initial report collection off the dispatcher and preserve the request's captured source identity. Read-only report generation must not invoke apply_fix or bypass confirmation.

## Separate debt
Tmux pipe drains currently read_to_end into Vec. A process deadline is not a byte budget; output resource bounds should be checked independently rather than mixed into moving report work off the UI thread.
