# Gboom feature audit

## Reachability and purpose
slash/commands/gboom.rs describes and implements a hidden shooter easter egg. visible returns false; commands/mod.rs registers it unconditionally. Bare invocation returns OpenGboom and arguments return a usage error. The dispatcher handles the Action; agent views store optional GboomState. This is active code on explicit invocation, not an unused or disabled diagnostic recorder.

## Integration
pager-render/src/gboom/{assets,engine,game,mod}.rs and render/gboom_overlay.rs contain 3251 source lines in the inspected working tree, including tests/comments. Assets are generated in code. GboomState caps frame dimensions at 480x320 and simulation dt at 0.1s. These are source facts, not measured CPU, frame rate or binary-size claims.

The pager reexports the renderer feature, stores per-agent game state, clocks simulation, clears held keys on focus/view changes, adjusts input coalescing, and pushes/pops a dedicated keyboard protocol layer. restore_terminal also pops that layer. Deletion must account for those exclusive hooks rather than merely removing the slash registration.

## Candidate boundary
R15 proposes removing the game module, overlay, hidden command, runtime action, state, exclusive scheduling/input/protocol hooks and associated tests if the user accepts. Preserve general image/kitty graphics support, ordinary keyboard protocol handling, terminal restoration, common animation clocks, input focus/cancellation semantics, and every real diagnostic command. Shared dependencies require reference revalidation before any removal.

## Validation and limits
Source audit only. Did not launch the game, modify runtime code, or run game tests. Hidden does not prove unnecessary: candidate status reflects its optional entertainment purpose, and the user decides. No quantified build-size saving or wholesale dependency deletion is promised. git diff --check and OpenSpec strict validation cover the new records.
