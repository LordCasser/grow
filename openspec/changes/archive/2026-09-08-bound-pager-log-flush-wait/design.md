# Evidence and design
pager/app/mod.rs awaits flush_blocking before restore_terminal and dropping AgentShutdownGuard. headless.rs likewise waits on exit. Forwarder::flush_blocking awaits acp_send, whose oneshot receiver has no deadline. A peer can retain AcpArgs forever while the channel remains open.

Use Tokio timeout of two seconds around the existing acp_send future. This is a best-effort diagnostic budget rather than a business request deadline. Timeout drops only the local wait; already enqueued notification may still be ingested, so do not retry or report that the remote work was canceled. No diagnostic logging from this failure path to avoid recursive log emission.

Test the actual production timeout with a fake ACP receiver that retains response_tx beyond completion, an outer watchdog, then verify the response receiver was dropped. Preserve acknowledged delivery and closed-peer behavior. These tests use isolated Forwarder and channels, no global logger or real disk.

# Adjacent findings
Historical detached batches are still not included by flush_blocking and queue byte budgets remain separate. Leader reconnection preserves the upper ACP sender and swaps leader_tx_shared inside leader_bridge; there is no evidence requiring runtime-owner rebinding here.
