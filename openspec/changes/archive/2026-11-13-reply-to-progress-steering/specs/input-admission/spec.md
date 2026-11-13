## ADDED Requirements

### Requirement: Progress replies to steering continue the admitted turn

A steer delivered to an active Turn SHALL allow a visible assistant progress reply to continue the same admitted work. The model SHALL be able to distinguish an interim reply from a terminal answer across supported provider protocols. An interim reply SHALL use the current Turn and its existing limits and SHALL NOT attempt plan completion or alter delivery obligations.

#### Scenario: Progress question while required delivery is outstanding

- **WHEN** the user asks for progress through a steer while required delivery remains unresolved and the model gives an interim reply
- **THEN** Grow displays and retains the reply, advances the same Turn without checking planning completion, and keeps the existing delivery obligations.

#### Scenario: Actual final after an interim progress reply

- **WHEN** the same Turn later attempts an actual final while required delivery is still unresolved
- **THEN** the existing planning completion gate applies, including stopping on unchanged blockers after a previous completion reminder.
