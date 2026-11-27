## ADDED Requirements

### Requirement: Live resumed child history loads without blocking interaction
Pager SHALL project a live resumed child's immutable inherited transcript off the interface thread while the parent, child and siblings remain interactive. The inherited display precedes the live tail and SHALL be installed once for the matching child binding.

#### Scenario: Live output arrives while inherited history is being read
- **WHEN** a newly resumed child receives its first prompt, produces output or finishes before its inherited display is ready
- **THEN** the live updates remain available and retain their identities and order when the inherited prefix is installed.

#### Scenario: Child binding retires or history fails
- **WHEN** a delayed inherited history result arrives after the owning view is retired or the journal cannot be read
- **THEN** another binding cannot acquire the result and a read error leaves the child's live output available with a retry path.
