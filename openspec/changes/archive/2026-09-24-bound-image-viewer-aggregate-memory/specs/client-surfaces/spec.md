## ADDED Requirements

### Requirement: Image viewer loading owns an aggregate memory reservation

Background image viewer loads SHALL acquire a process-wide reservation before copying encoded source bytes and before allocating conversion workspace or output. The reservation SHALL cover conservatively budgeted in-process conversion memory and the actual retained encoded buffers, and SHALL remain owned by a loaded viewer or undelivered result until that owner is dropped. Admission failure SHALL complete the current viewer as a failed preview without blocking the input thread or installing partially loaded bytes.

#### Scenario: Concurrent viewers approach the process allowance

- **WHEN** retained viewer buffers and in-flight conversions would exceed the aggregate allowance
- **THEN** the next load fails before its unreserved allocation, while existing viewers remain usable and input remains responsive.

#### Scenario: Viewer closes or a stale result is discarded

- **WHEN** a viewer closes/reopens or a delayed background result no longer matches its target owner
- **THEN** its reservation is released with the dropped viewer/result, and that result cannot replace the current viewer.

#### Scenario: Conversion fails after admission

- **WHEN** decoding or conversion fails after source admission
- **THEN** temporary reservation is released and the viewer settles through the existing failed-preview path.
