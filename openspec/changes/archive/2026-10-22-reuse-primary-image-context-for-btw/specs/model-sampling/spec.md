## MODIFIED Requirements

### Requirement: Side questions reuse durable image fallback
/btw SHALL be a read-only consumer of the primary session's image projection and SHALL use its prepared primary LLM route. Before freezing Sideband context it SHALL wait for any in-progress primary image conversion, including its durable ImageProjection acknowledgement. It SHALL select every available image description from that acknowledged Surface on the request copy, independently of the negative-capability cache. It SHALL NOT start vision/OCR/conversion work, write negative image capability or retry an image rejection by mutating the primary Surface. The question and answer SHALL remain Sideband-only. Waiting SHALL observe session Sideband cancellation and SHALL NOT hold the image projection gate during its provider request or overload retries. An unsupported transport with unresolved images SHALL fail without issuing an image-bearing request; unconverted media for an unknown transport retains existing optimistic semantics.

#### Scenario: Known text-only route
- **WHEN** main context retains original images paired with acknowledged descriptions and /btw is requested
- **THEN** its frozen portable context uses the existing descriptions without raw media or another auxiliary request, including when negative capability is absent

#### Scenario: Primary conversion has not finished
- **WHEN** main vision/OCR recovery or its ImageProjection acknowledgement is pending and /btw is requested
- **THEN** /btw waits without starting a provider or conversion request, then freezes and uses the acknowledged text branch once main conversion completes

#### Scenario: Unrelated failure or failed projection
- **WHEN** main conversion cannot commit a usable text branch or the waiting SideQuestion is cancelled
- **THEN** /btw returns failure without independently converting images or emitting an unacknowledged textual provider request

#### Scenario: No main conversion owns unresolved images
- **WHEN** the prepared transport is known text-only but main context has unresolved images and no conversion is in progress
- **THEN** /btw fails as context not ready without triggering recovery or modifying image capability

#### Scenario: Historical images rejected by side question
- **WHEN** its frozen request receives an image rejection or ordinary 400
- **THEN** it records the Sideband failure and returns it without starting shared image recovery or writing negative capability

#### Scenario: Foreground context advances during the failed request
- **WHEN** /btw's frozen provider request fails while main context advances
- **THEN** /btw returns the failure without projecting either its frozen images or images from the newer Surface
