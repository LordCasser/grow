## ADDED Requirements

### Requirement: Skill metadata omits inactive sampling overrides
Skill discovery and RPC metadata SHALL NOT expose unused model or effort override fields. Session model selection and reasoning effort SHALL remain independent.

#### Scenario: Skill metadata advertisement
- **WHEN** a skill is discovered and advertised through RPC
- **THEN** no skill-level model/effort override is advertised.

### Requirement: Skill inventory has one runtime owner
SkillManager SHALL remain the source for merged startup/discovered skill listings and slash advertisement without a parallel AvailableSkills resource snapshot.

#### Scenario: Dynamic discovery retains startup skills
- **WHEN** a new skill is discovered after registry initialization
- **THEN** SkillManager retains both startup and discovered skills in its active listing.
