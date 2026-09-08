# Design
The CI failure was reproduced with only async_compaction_authority_transition_cancels_before_publication running. Its Builder explicitly requests 8 MiB, so macOS default stack and RUST_MIN_STACK are not the controlling settings.

LLDB catches the production loop poll prologue reserving 0x24b000 + 0x6f0 bytes. Temporary generic type-size instrumentation measured model_switch=131760, background=131496, image_projection=129488, tool_loop=32104 and loop_body=136272 bytes. The unoptimized poll frame is much larger than the future state. Boxing the outer call sites and then the instrumented loop body did not solve the overflow; both experiments and size instrumentation were removed.

Use the same 32 MiB test stack as existing image_input_recovery/truncation/chat_history_integrity harnesses. Keep production SESSION_THREAD_STACK_SIZE=8 MiB and all behavioral assertions unchanged. Validate the full compaction suite, then other explicit-8-MiB harnesses and the full core suite. A production loop redesign is outside this CI fix.
