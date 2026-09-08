# Release preparation
Release execution uses the existing official GitHub Actions workflow. Publication is not implied by completion of this metadata preparation change. CI run URLs and final release status will be reported from actual remote results.

Cleanup archived with 272/272 archive checks; per-item regression evidence is in clean-approved-milestone-candidates. cargo clean removed 8.7 GiB, leaving 52 GiB available. Workspace version is 2.1.5; exactly 33 local lockfile package versions changed, with no source/dependency/checksum changes. Full cargo metadata --locked --offline resolved successfully. Notes describe bounded fixes and retained candidates without claiming all audits complete.
