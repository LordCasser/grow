## ADDED Requirements

### Requirement: Crash address symbolication requires matching image provenance

Crash recovery SHALL not resolve captured absolute instruction pointers using the recovering process's current address map unless matching captured image and load provenance has been established. For capture formats without that provenance, reports SHALL preserve every raw instruction pointer and explicitly identify automatic symbolication as unavailable, without inventing function names or file locations. PID or version equality alone SHALL not establish matching address-space provenance. Existing report and raw evidence durability SHALL remain in effect.

#### Scenario: Old IP overlaps a current function
- **WHEN** an unverified captured address happens to point into a known function of the recovering process
- **THEN** recovery retains the original raw address but does not attribute the crash to that current function.

#### Scenario: A PIE process restarts with another slide
- **WHEN** capture and recovery execute with different address-space slides and the capture lacks module/load metadata
- **THEN** the report preserves captured addresses and explains the missing provenance rather than attempting current-map symbolication.
