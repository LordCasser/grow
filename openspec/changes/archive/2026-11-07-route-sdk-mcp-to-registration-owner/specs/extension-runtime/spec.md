## ADDED Requirements

### Requirement: SDK MCP reverse requests retain their registration connection

An SDK MCP registration SHALL bind its reverse invoker to the registering session and connection. Leader ingress SHALL derive the connection identity from its actual accepted connection and overwrite any caller-supplied routing identity on session creation or load. Reverse SDK requests SHALL carry the bound session and, in leader mode, that connection target. The existing target route SHALL take precedence over the current session driver, and an unavailable registration owner SHALL NOT cause fallback to a driver, a same-serverId registration or the last active client. Standalone single-connection ACP SHALL continue using its own gateway.

#### Scenario: Another connection is the session driver

- **WHEN** connection A owns an SDK registration while connection B is the session's driver
- **THEN** SDK initialize, list and tool calls continue to target A while ordinary driver-owned reverse requests retain their existing routing to B.

#### Scenario: Another connection loads a resident session

- **WHEN** connection B loads a still-resident session whose SDK tools were registered by A
- **THEN** the existing SDK registration and invoker remain bound to A, and the load retains the existing join and driver policies.

#### Scenario: Two registrations use the same server identifier

- **WHEN** A and B each register an SDK server using the same serverId in their own sessions
- **THEN** each reverse exchange targets the connection bound to that registration without using serverId to select a connection.

#### Scenario: Registration owner disconnects

- **WHEN** A disconnects while B remains the session driver and an SDK request is made through A's still-bound registration
- **THEN** B receives no SDK request and the exchange fails under the existing gateway or timeout semantics without silently rebinding the registration.

#### Scenario: A caller supplies a different connection identity

- **WHEN** a new/load request supplies a forged routing owner or leaves optional metadata absent or null
- **THEN** the leader forwards valid requests with its actual accepted connection identity.

#### Scenario: Session metadata cannot carry an injected owner

- **WHEN** a new/load request supplies non-null metadata that is not an object
- **THEN** the leader rejects the request with invalid params to the originating connection before changing session routing or pending-load state, does not forward it to the agent, and does not normalize it into a successful ownerless request.

#### Scenario: Cold load creates a new registration

- **WHEN** a session is cold loaded on a new connection with fresh SDK declarations
- **THEN** its newly created invoker binds the current registering connection and does not restore a prior process's ClientId.

#### Scenario: A child inherits an already connected SDK client

- **WHEN** the existing delegated MCP ceiling permits a child to inherit a connected SDK client
- **THEN** that shared client retains the original registration invoker and owner without widening delegation or using the child's current driver as the SDK owner.

#### Scenario: Direct ACP has one connection

- **WHEN** SDK registration is created through standalone ACP without a leader target identity
- **THEN** reverse requests use that connection's own gateway and retain the existing bounded response behavior.
