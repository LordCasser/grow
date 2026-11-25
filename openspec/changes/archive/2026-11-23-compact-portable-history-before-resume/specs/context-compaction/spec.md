## ADDED Requirements

### Requirement: Automatic compaction maintains room for portable replay

During ordinary execution, preventive automatic compaction SHALL consider both the calibrated current request and the provider-neutral history a cold resume would replay under the committed Goal scope. Either input reaching the existing threshold qualifies. The portable estimate SHALL exclude omitted reasoning, incomplete tool exchanges and request-only fields. An explicit output reservation SHALL continue to apply to the current request. Presentation of current context, usage accounting and hard provider admission SHALL use their existing current-request evidence.

#### Scenario: Portable tool history grows while provider usage stays low

- **WHEN** complete tool exchanges grow across ordinary Steps, the provider reports a smaller context, and portable replay reaches the background or synchronous threshold
- **THEN** the existing background or synchronous compaction path handles the eligible range, with one owner for a pending generation. Foreground feedback identifies replay-driven maintenance. A cold resume replays its committed replacement and retained tail.

#### Scenario: Pruning or image projection changes the evidence

- **WHEN** automatic compaction durably prunes tool results or changes image content before deciding whether to summarize
- **THEN** it rechecks the applicable thresholds on the resulting history and current request. A low provider observation alone cannot declare the portable history small enough.

#### Scenario: Portable pressure alone cannot close current provider admission

- **WHEN** only the portable estimate exceeds the window and there is no eligible range, including when the oldest complete tool exchange cannot fit the summary input budget
- **THEN** the otherwise fitting current request may proceed through ordinary admission under the existing suppression policy. An actual current request overflow or provider rejection still follows its hard recovery boundary.

#### Scenario: A committed summary retains a large recent exchange

- **WHEN** a summary or promotion commits before an ordinary request and the retained tail still crosses the portable threshold
- **THEN** the same request proceeds to its current-input and output-reserve check without another replay-only summary. Subsequent model work can create new evidence for preventive compaction.

#### Scenario: Omitted content and suppressed work

- **WHEN** the large part of history is omitted from cold replay, or ordinary automatic compaction is suppressed
- **THEN** that omitted content or a portable estimate alone does not start a summary outside the normal automatic compaction policy.
