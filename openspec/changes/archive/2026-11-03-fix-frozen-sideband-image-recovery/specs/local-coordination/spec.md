## MODIFIED Requirements

### Requirement: Inquiry snapshots reuse durable image descriptions

Parent-child and peer inquiries SHALL bind client, request model and image capability to one prepared receiver route. They SHALL freeze acknowledged context after cancellably waiting for primary image conversion, prefer all available descriptions and release the projection gate before answer sampling. Known rejecting routes SHALL recover selected unresolved images before provider admission. Explicit image rejection on an unknown route SHALL record that actual route's negative capability and permit one neutral text recovery attempt. Recovery SHALL use only the original frozen source, SHALL NOT mutate the primary Surface, and SHALL fail if descriptions are unavailable or the admitted budget is exceeded. Ordinary errors SHALL NOT trigger image recovery. Inquiry and session cancellation SHALL cover recovery and retry.

#### Scenario: Switch from a multimodal model to a text model
- **WHEN** a child or peer inquires after the receiver switches to text with unresolved historical images
- **THEN** the inquiry recovers neutral descriptions before admission on a known route or once after explicit rejection on an unknown route, then answers without raw images.

#### Scenario: Inquiry is cancelled while waiting
- **WHEN** inquiry or session cancellation occurs during projection waiting or recovery
- **THEN** no further answer request is emitted and the inquiry terminates as cancelled.

#### Scenario: Context advances during recovery
- **WHEN** new input reaches the receiver while its frozen inquiry recovers images
- **THEN** its answer excludes the new input and recovery leaves the primary Surface intact.

#### Scenario: Conversion awaits durable acknowledgement
- **WHEN** the primary image projection awaits its durable acknowledgement
- **THEN** inquiry capture waits and uses the acknowledged descriptions without holding the gate during answer sampling.

#### Scenario: Text-only route lacks a committed description
- **WHEN** the prepared receiver route is known text-only and selected images are unresolved
- **THEN** neutral recovery precedes answer admission and missing descriptions fail without primary Surface mutation.

#### Scenario: Unknown transport rejects unresolved images
- **WHEN** an unknown route explicitly rejects the image-bearing inquiry
- **THEN** that actual route is learned and one text recovery retry uses the original frozen sources.
