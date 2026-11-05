## ADDED Requirements

### Requirement: LSP diagnostic attribution observes admitted document revisions

LSP document state SHALL publish the revision and UTF-16 end position of a didOpen or didChange notification atomically with successful local transport admission. A diagnostic handler observing the update's response SHALL see that admitted revision; it SHALL NOT permanently attribute the response to an older or unopened document because admission succeeded before revision publication. Failed notification admission SHALL preserve the previous document state. The synchronous admission boundary SHALL NOT wait for network responses or hold its state lock across asynchronous work.

#### Scenario: Fast push follows an admitted open

- **WHEN** didOpen admission succeeds and a versioned or versionless pushed report arrives before the sending caller resumes
- **THEN** diagnostic attribution waits for the admitted document state and can settle the first edit instead of recording an unopened-document verdict.

#### Scenario: Fast push follows an admitted change

- **WHEN** didChange admission succeeds for a newer revision and its pushed report races the local update publication
- **THEN** the report is attributed against the newly admitted revision and remains available to the pending diagnostic reader.

#### Scenario: Notification admission fails

- **WHEN** the transport rejects didOpen or didChange admission
- **THEN** the client does not advance tracked version, language or end position, and the next incremental replacement is computed from the last successfully admitted text.

#### Scenario: A stale or unsolicited report arrives

- **WHEN** a report describes an older revision, a document never admitted, or a revision beyond what was admitted
- **THEN** existing revision and unopened-document boundaries remain in effect; the admission fix does not make those reports settle an unrelated edit.
