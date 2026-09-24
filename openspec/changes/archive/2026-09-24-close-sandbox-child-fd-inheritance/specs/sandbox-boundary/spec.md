## ADDED Requirements

### Requirement: Restricted Linux children admit only owned descriptors at exec

When a known Linux child launch applies the sandbox child-network filter, it SHALL mark every descriptor above standard error close-on-exec before installing the filter. The launch SHALL fail if the close-on-exec boundary cannot be established. Only Grow-owned shell-state pipes explicitly mapped by that launch MAY survive exec as non-standard descriptors; standard input, output, and error retain their configured ownership.

#### Scenario: Pre-connected TCP or UDP socket inherited from Grow

- **WHEN** Grow holds a connected TCP or UDP socket without close-on-exec and launches a network-restricted child
- **THEN** the execed child cannot send over that descriptor, including with ordinary file-writing syscalls.

#### Scenario: Shell-state pipe mappings

- **WHEN** a restricted static or persistent shell launch maps Grow-created state pipes to fd 3 or fd 3 and fd 4
- **THEN** those mapped pipes remain usable after exec while every other non-standard descriptor is closed.

#### Scenario: Descriptor admission unavailable

- **WHEN** the kernel rejects the close-on-exec range operation or an expected state pipe is absent
- **THEN** child spawn fails before exec and the network filter does not silently provide a weaker guarantee.
