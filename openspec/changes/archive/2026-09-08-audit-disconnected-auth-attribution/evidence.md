# Evidence

- Repository-wide Rust search for Auth401AttributionCallback implementations and record_401: only CountingCallback inside sampler client #[cfg(test)] implements the trait. The sole Some callback assignment is that test.
- SamplerConfig default, sampler actor fixture/config initialization, shell sampling_config_for_model: None. stamp_session_local_sampler_fields and workflow tracker clone/restore a preexisting optional callback, never create one. Serde skips the field.
- sampler/client.rs::record_401_attribution gates on Some and is called at six 401 sites. No production implementation means no repository runtime logging sink is proven through this callback.
- SentRequest.sent_bearer also feeds auth_rejected -> SentCredential::from_sent_fragment. shell/session/actor/auth_retry.rs::on_recovered_401 and turn/sampling.rs consume credential.is_missing; this is active retry behavior and excluded from R32.
- current_sent_bearer_prefix feeds auth_info's auth-type detection, plus tests. Do not delete it with callback fields without replacing its active consumer.
- attribution.rs previously referenced absent shell token_suffix implementations and claimed full credentials never leave the sampler. Existing bearer_tail_fragment tests explicitly return abc unchanged. Comments now accurately describe short-key exposure without changing code.

Limit: repository reachability only, not proof of absent external library clients. No production callback was added or removed, and no real credentials/logs were read.
