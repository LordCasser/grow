## MODIFIED Requirements

### Requirement: Image normalization uses the bounded compute path

Image normalization SHALL run through its cancellation-safe worker admission without an inactive process-wide normalization cache or its unused remote activation flag. Image format conversion, integrity checks, original-content fallback and attachment metadata SHALL retain their existing behavior. Full pixel decode SHALL reject source images above 50,000,000 pixels before allocating their pixel buffer. One normalization batch SHALL admit at most 25 images and 80,000,000 bytes of encoded image data before awaiting compute. Concurrent normalization batches SHALL reserve at most 160,000,000 encoded bytes process-wide; a batch that cannot reserve capacity SHALL promptly drop its images instead of retaining them as unbounded waiters. A running blocking worker SHALL retain its batch reservation after caller cancellation until that work exits. Drop outcomes SHALL remain attributable to individual input indexes but SHALL be grouped by identical reason before rendering; one normalization batch SHALL produce at most one model reminder and one `ImageDropped` update, with each distinct reason rendered once and all affected indexes listed in stable input order.

#### Scenario: Normalize repeated attachments

- **WHEN** attachments with identical content are admitted
- **THEN** each uses the supported normalization path and produces the same valid content and per-attachment metadata without consulting a disabled cache.

#### Scenario: Cancel a normalization waiter

- **WHEN** a caller is cancelled after blocking normalization has started
- **THEN** the running worker retains both compute admission and its encoded-byte reservation until its actual work completes.

#### Scenario: Re-encoding cannot meet the bound

- **WHEN** normalization cannot produce an encoding under its byte limit for an admitted image
- **THEN** the original attachment and its indexed fallback notice remain available.

#### Scenario: Several images fail for the same reason

- **WHEN** one normalization batch drops multiple images for an identical integrity, dimension, pixel-count or admission reason
- **THEN** the model reminder and `ImageDropped` notes contain one summary line for that reason with every affected image index, and the client receives one NOTICE block rather than one repeated sentence per image.

#### Scenario: Images fail for different reasons

- **WHEN** one normalization batch drops images for more than one reason
- **THEN** each distinct reason appears on one line in first-occurrence order, indexes within each line preserve input order, and the user-attachment and tool-result paths use the same summaries.

#### Scenario: Too many images or encoded bytes

- **WHEN** a normalization batch contains more than 25 images or its encoded payload exceeds 80,000,000 bytes
- **THEN** excess images are released before compute starts, admitted images remain in input order, and excess indexes receive explicit dropped-image outcomes.

#### Scenario: Concurrent batches exhaust encoded-byte capacity

- **WHEN** a new batch would raise live normalization input reservations above 160,000,000 bytes
- **THEN** its admitted images are promptly dropped with indexed capacity reasons; no encoded payload waits uncharged for the compute worker.

#### Scenario: Source exceeds client decode budget

- **WHEN** an image above 50,000,000 source pixels requires re-encoding
- **THEN** the normalizer drops it before full decode even if it is below the provider's larger persisted-image validity ceiling.
