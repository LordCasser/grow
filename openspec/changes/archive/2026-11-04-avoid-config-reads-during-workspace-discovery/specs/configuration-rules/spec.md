## ADDED Requirements

### Requirement: Workspace discovery does not open Git configuration

Project configuration, Agent, skill and rule discovery SHALL determine checkout boundaries from Git directory topology without reading Git configuration. Normal repositories and linked or separate worktrees SHALL retain their own checkout roots through cwd aliases; bare repositories SHALL NOT become checkout roots. Existing home-directory exclusions SHALL remain in effect. Trust SHALL retain root, marker and common Git directory filesystem identities and SHALL reject invalid or unreadable required topology.

#### Scenario: Git configuration cannot be read
- **WHEN** a checkout has valid Git topology but `.git/config` is unavailable or malformed
- **THEN** configuration and Agent discovery still use the checkout boundary without opening that file, and trust identity still includes the original marker and common directory entities.

#### Scenario: Worktree topology fails validation
- **WHEN** a required gitfile or common-directory link is invalid or unreadable
- **THEN** discovery reports the existing failure and trust does not authorize a grant using an incomplete identity.

#### Scenario: Worktree and alias boundaries
- **WHEN** discovery starts in a nested or aliased directory of a linked or separate worktree
- **THEN** the boundary is that checkout's root, without widening to the common Git directory's parent or to another checkout.
