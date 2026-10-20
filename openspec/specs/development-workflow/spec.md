# development-workflow Specification

## Purpose
定义本项目从此次迁移起采用的 OpenSpec SDD 开发流程。明确当前规范、开发过程和开发者文档的职责，约束变更前的场景设计、最小非行为变更与无关架构债务处理；这是新增治理契约。

## Requirements

### Requirement: Specification authority
项目开发 SHALL 将 openspec/specs 作为已归档行为契约，将 openspec/changes 作为变更过程记录，将 docs 作为开发者阅读说明。

#### Scenario: 文档冲突
- **WHEN** docs、历史审计与当前 spec 描述不一致
- **THEN** 先核对代码证据，在 change 中记录差异并修正规范或实现；不把历史计划直接当成已实现事实。

证据：`AGENTS.md` — `OpenSpec`。

### Requirement: Spec first implementation
改变行为、接口、状态、安全或持久化契约的开发 SHALL 先形成 proposal、delta specs、design 和 tasks，再实施与验证。

#### Scenario: 开始实现行为变化
- **WHEN** 已识别受影响 capability 与调用方
- **THEN** 先写可验证 WHEN/THEN 场景，完成实现和相关验证后归档更新主规范。

证据：`AGENTS.md` — `OpenSpec`。

### Requirement: Minimal nonbehavior change
不改变行为的纯文档、工具维护或纯重构 SHALL 保持最小 change，并在 .openspec.yaml 显式设置 skip_specs: true；不得为通过校验虚构产品需求。

#### Scenario: 修正文档错字
- **WHEN** 确认现有契约不变
- **THEN** 简述理由、影响与验证，不增加无意义 delta；仍保留可追溯 change 记录。

证据：`AGENTS.md` — `skip_specs`。

### Requirement: Separate architecture debt
审计发现的无关架构债务 SHALL 单独登记，不扩入当前实现；长期计划不自动构成开发授权。

#### Scenario: 审计发现额外问题
- **WHEN** 问题与当前 change 验收无关
- **THEN** 登记到 OpenSpec backlog，待明确启动后单独建立 change。

证据：`AGENTS.md` — `backlog`。

### Requirement: Release regression gates are bound to the published source
The release workflow SHALL require its relevant regression suite to succeed on the exact commit validated from the release tag before publishing assets.

#### Scenario: Publish a validated release
- **WHEN** the release workflow validates a tag and publication is requested
- **THEN** it SHALL run reusable regression gates and strict OpenSpec validation against the validated commit, and publish only if validation, build, dispatch guard, and every required gate completed successfully.

#### Scenario: A regression gate does not complete successfully
- **WHEN** a required gate fails, is skipped, or is cancelled
- **THEN** the release workflow SHALL NOT publish assets.

#### Scenario: The release workflow is cancelled after its dependencies finish
- **WHEN** the release workflow is cancelled after its dependencies have completed
- **THEN** the release workflow SHALL NOT publish assets.

### Requirement: Exact Cargo regression selections prove execution
CI SHALL reject exact Cargo test selections that are missing, ambiguous, fail, or execute no test.

#### Scenario: A selected test exists exactly once
- **WHEN** CI selects an exact test name and its ignored status is explicitly requested when applicable
- **THEN** CI SHALL verify one matching test is listed and exactly one test executes successfully.

#### Scenario: A selected test name is stale or does not execute
- **WHEN** an exact selection has no unique listed test or Cargo reports zero executed tests
- **THEN** the regression gate SHALL fail.

### Requirement: Official release binaries identify the validated source
Official release builds SHALL embed the version and seven-character source prefix from the validated immutable release commit across native and container build boundaries. Explicit malformed source metadata SHALL fail the build; release smoke SHALL verify the complete expected version identity.

#### Scenario: Container Git metadata is unavailable
- **WHEN** an official builder receives the validated full RELEASE_COMMIT but Git inside the build environment is missing or cannot read the checkout
- **THEN** the binary reports that commit's seven-character prefix with the validated version rather than unknown

#### Scenario: Explicit build identity changes or is malformed
- **WHEN** RELEASE_COMMIT changes on an existing Cargo target directory or is present but not a full hexadecimal commit
- **THEN** Cargo rebuilds the identity for a valid new value, and malformed input fails compilation without a silent Git/unknown fallback

#### Scenario: Release smoke sees a wrong source identity
- **WHEN** a smoke-tested release binary has the right product version but a missing or wrong source prefix
- **THEN** the smoke gate fails and publication remains blocked

#### Scenario: Ordinary local build without explicit metadata
- **WHEN** RELEASE_COMMIT is absent in a local development build
- **THEN** existing Git-based identity discovery and the Git-unavailable unknown fallback remain available

### Requirement: Exact Cargo regression runners expose bounded phases
The exact Cargo regression runner SHALL distinguish compilation, test enumeration, and test execution; stream command output while a phase is running; and enforce configurable phase deadlines.

#### Scenario: A selected test is compiled and executed
- **WHEN** the runner starts an exact test selection
- **THEN** it SHALL report compilation, enumeration, and execution as separate phases, stream each phase's output as it arrives, and retain exact-name and one-successful-test checks.

#### Scenario: A Cargo phase exceeds its deadline
- **WHEN** compilation, enumeration, or test execution exceeds that phase's configured deadline
- **THEN** the runner SHALL fail with the phase identified, attempt to terminate the command's process tree, and return only after cleanup is confirmed or explicitly reported as unconfirmed.

#### Scenario: The runner is cancelled during a Cargo phase
- **WHEN** the runner receives an interrupt while a Cargo phase is active
- **THEN** it SHALL attempt to terminate the command's process tree, reap the command process, report cancellation and whether cleanup was confirmed, and exit unsuccessfully.

#### Scenario: A descendant keeps an inherited output pipe open after its parent exits
- **WHEN** a Cargo phase's direct child exits but a descendant keeps the merged output pipe open past the phase deadline
- **THEN** the runner SHALL finish cleanup waiting within a bounded interval and fail with an explicit cleanup-unconfirmed result if it cannot confirm the tree has stopped.

#### Scenario: A configured deadline is not finite and positive
- **WHEN** a phase deadline is zero, negative, NaN, or infinite
- **THEN** the CLI SHALL reject the value before starting Cargo.

#### Scenario: The output reader fails while the command exits successfully
- **WHEN** the output pipe reader encounters an I/O error during a Cargo phase
- **THEN** the runner SHALL report the read failure and SHALL NOT treat it as clean end-of-file, even if the command exits successfully.

#### Scenario: Exact selection is absent, ignored without opt-in, or executes no passing test
- **WHEN** enumeration is non-unique, an ignored test was not explicitly enabled, or execution does not prove exactly one successful non-ignored test
- **THEN** the runner SHALL fail even when Cargo exits successfully.

### Requirement: Exact Cargo selections stay pinned to one compiled test executable

Exact Cargo test selection SHALL compile the selected package/target once, identify the unique test executable from Cargo JSON compiler-artifact messages where `profile.test` is true and `executable` is nonempty, and invoke that same executable directly for test enumeration and execution. Cargo diagnostics and progress SHALL remain readable; artifact JSON SHALL NOT obscure the bounded diagnostic tail. Missing or ambiguous executable selection SHALL fail closed. Existing exact-name, ignored-test opt-in, successful execution, phase deadline and process-tree cleanup checks SHALL remain in force.

#### Scenario: One compiled artifact serves all exact-test phases
- **WHEN** a Cargo no-run build produces exactly one test executable
- **THEN** the runner enumerates and executes the selected test through that same executable without invoking Cargo again, and still proves one exact match and one successful test.

#### Scenario: Test executable is missing or ambiguous
- **WHEN** Cargo produces no eligible test executable or produces more than one unique eligible executable
- **THEN** the runner fails before enumeration and execution.

#### Scenario: Artifact JSON is streamed without hiding diagnostics
- **WHEN** the compile stream contains compiler artifacts and compiler messages
- **THEN** artifacts are captured independently of diagnostic-tail size, rendered diagnostics and Cargo progress remain visible, and compiler messages without rendered text retain their raw content.

#### Scenario: Test executable keeps its package working directory
- **WHEN** the selected package is a member below the Cargo workspace root
- **THEN** enumeration and execution run with the package manifest's parent directory as their working directory.
