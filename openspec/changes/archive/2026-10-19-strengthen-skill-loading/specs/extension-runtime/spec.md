## ADDED Requirements

### Requirement: Skill invocation resolves identity before eligibility
Grow SHALL resolve a skill name against its current catalog before checking enabled and caller eligibility. A disabled selected identity SHALL produce an explicit cannot-load error and SHALL NOT fall back to another source. Model guidance SHALL require load_skill for using skills and require reporting failure without bypass through file reads, shell commands, or alternate copies. Ordinary file inspection SHALL remain distinct from activation.

#### Scenario: Disabled name collides with an enabled plugin
- **WHEN** a disabled native skill shares a bare name with an enabled plugin skill
- **THEN** the bare name is ambiguous, the disabled qualified name fails explicitly, and only explicitly selecting the enabled qualified name can load it.

#### Scenario: Disabled or unavailable skill is requested by a role
- **WHEN** a model requests a disabled, missing, or caller-ineligible skill
- **THEN** the loader reports why it cannot load the skill, publishes no guidance, and instructs the model to report the failure without attempting another access route.

### Requirement: Skill discovery advertises names without eager bodies
Grow SHALL use its existing startup and refresh catalog as the only skill index. Model-facing discovery SHALL advertise names and bounded descriptions without installation paths or eagerly loaded bodies. Names SHALL remain usable by load_skill without supplying a filesystem path.

#### Scenario: Large skill with optional references
- **WHEN** discovery indexes a skill with a long body and several reference documents
- **THEN** the catalog contains only its bounded metadata and the resources are not loaded into model context.

#### Scenario: Preloaded skill needs a reference
- **WHEN** an Agent initialization declaration loads a skill body
- **THEN** its catalog identity remains available for later name-based resource loads.

### Requirement: Skill resources load progressively through the same tool
load_skill SHALL accept an optional resource relative to the selected skill root. Without resource it SHALL load only the main skill instructions. With resource it SHALL load only that UTF-8 file through normal authorization, cancellation and durable SkillLoad publication, without executing scripts, expanding templates, or recursively loading references. Resources SHALL respect the same enabled/caller policy, 1 MiB source bound and gitignore policy as main loads, and SHALL reject args combined with resource.

#### Scenario: Main instructions followed by a needed reference
- **WHEN** the model first calls load_skill by name and later requests references/api.md
- **THEN** the first result contains only the main instructions and the second contains only that reference with its skill/resource identity; neither includes sibling files.

#### Scenario: Resource cannot be combined with template arguments
- **WHEN** a load_skill request supplies both a resource and args
- **THEN** the request fails before reading or publishing either body; resource reads do not apply skill template arguments.

#### Scenario: Invalid or escaping resource
- **WHEN** resource is absolute, contains parent traversal, has a Windows drive or UNC spelling, or resolves through a symlink outside the skill root
- **THEN** loading fails with no resource guidance published and no fallback path read.

#### Scenario: Resource permissions and file identity
- **WHEN** a resource request reaches authorization or its resolved path changes before execution
- **THEN** authorization covers the actual resource file and changed identity fails before reading instead of reusing permission for SKILL.md.

#### Scenario: Missing, oversized, non-UTF-8 or cancelled resource
- **WHEN** the resource is unavailable, exceeds 1 MiB, is not UTF-8 or preparation is cancelled
- **THEN** loading fails without publishing a partial body or claiming Loaded.

#### Scenario: Skill has no accessible resource root
- **WHEN** a resource is requested from a URI-backed or frozen-only skill without an accessible local resource root
- **THEN** loading returns an explicit resource-unavailable error without resolving a fallback path or publishing content.

#### Scenario: Resource result survives suspension
- **WHEN** a resource result was durably admitted before suspension
- **THEN** replay uses the committed resource body and existing SkillLoad identity without reopening the current file or recursively loading more material.

### Requirement: Skill file inspection follows ordinary read bounds
Reading SKILL.md or Markdown under a skills directory through a file tool SHALL remain ordinary file inspection, honoring requested line windows and the normal token cap. Full skill guidance SHALL be loaded through load_skill and its explicit preparation/publication boundaries.

#### Scenario: Inspect a line of a skill for editing
- **WHEN** a file read requests a line window of SKILL.md or a reference document
- **THEN** only the requested bounded window is returned and no Skill Loaded fact is created.

#### Scenario: Oversized skill file inspection
- **WHEN** an ordinary skill file read exceeds the normal token cap
- **THEN** the same size error as other text files is returned; the skill filename does not bypass read limits.

#### Scenario: Resource call crosses the durable publication boundary
- **WHEN** a successful resource load is committed for a model-issued load_skill call
- **THEN** Timeline accepts the resource argument only when the current call matches the uniquely admitted call, including the resource; unknown fields or a changed resource cannot publish guidance.
