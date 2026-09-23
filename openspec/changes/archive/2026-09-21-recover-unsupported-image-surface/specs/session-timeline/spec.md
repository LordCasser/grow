## MODIFIED Requirements

### Requirement: Image descriptions retain their original image evidence

An acknowledged `ImageProjection` SHALL retain original image payloads in immutable Timeline message evidence while advancing the affected current-branch Surface identities. Each image shadow SHALL carry an exact source fingerprint/count and a typed disposition: provider description and local OCR dispositions SHALL retain the image with a nonempty reusable description, while unsupported-model disposition SHALL remove the image from the materialized Surface and insert the canonical replacement `当前模型不支持多模态，图片已经被删除`. Live apply and bulk replay SHALL validate and produce identical Surface, compaction-reference and image-tool-path redaction results. Local OCR SHALL identify its engine, provider descriptions SHALL reference a valid ImageDescription Sideband, and unsupported-model replacement SHALL require the exact canonical text rather than arbitrary unproven content.

#### Scenario: Resume a described image

- **WHEN** a session with a completed provider description or local OCR projection is replayed
- **THEN** the current Surface contains the image and its description with consistent causal Surface identities, and immutable Timeline evidence remains available.

#### Scenario: Resume a removed unsupported image

- **WHEN** a session with an acknowledged unsupported-model image projection is replayed
- **THEN** the original message event still contains the raw image as evidence, while the current Surface contains one canonical replacement at that image group's causal position and no raw image from the group.

#### Scenario: Select request representation

- **WHEN** a known unsupported canonical provider/model pair prepares a request after successful description or OCR projection
- **THEN** the request uses available descriptions without mutating the retained image.

#### Scenario: Unresolved group is removed atomically

- **WHEN** an exact-revision projection contains both description-backed groups and unsupported-model groups
- **THEN** Timeline validates every source, fingerprint, count and disposition before accepting one event, then atomically attaches descriptions and removes unresolved Surface images.

#### Scenario: Image-bearing tool result is removed

- **WHEN** unsupported-model projection targets an image-bearing tool result
- **THEN** replay removes the result images, retains one canonical replacement in the same causal item and applies the validated tool-call, response-carrier and compaction-reference redactions so no model-visible path can re-inject the image.

#### Scenario: Projection persistence is not acknowledged

- **WHEN** a prepared image projection is invalid, cannot be durably written or has an unconfirmed acknowledgement
- **THEN** the accepted Surface remains unchanged and sampling cannot claim removal or resubmit from a temporary request copy.

#### Scenario: Switch to an unknown pair

- **WHEN** another canonical provider/model pair prepares its first request after a description-backed projection
- **THEN** original image parameters remain available even if another pair previously selected descriptions.

#### Scenario: Removed evidence is not implicitly resurrected

- **WHEN** another model prepares a request after an unsupported-model projection removed an image from the current Surface
- **THEN** request assembly reads the projected Surface and does not recover raw media from historical Timeline evidence without an explicit branch operation.
