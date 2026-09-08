# Verification

## Contract coverage
- Original image plus optional group description survives serialization, Timeline replay, Surface replacement, rewind and compaction recall. Invalid OCR engine/source/fingerprint bindings are rejected.
- Request-only description selection validates all groups before changing any request item. Original image parts remain available for unknown canonical provider/model pairs; marked pairs reuse descriptions without another auxiliary call. Native continuation reconciliation and byte budgeting run after selection.
- Mock provider recovery covers primary image rejection, successful visual auxiliary conversion and text resubmission, auxiliary rejection, preserved images on failure, new-provider first image attempt and existing-description reuse.
- Ordinary description/OCR failure is not a fatal turn boundary. The user receives unsupported-multimodal and manual rewind guidance. Failed-turn events replay successfully and manual rewind remains usable; no incomplete ImageProjection is installed.
- Local OCR tests cover bounded inline decoding, stdin EOF, successful/empty/failed/oversized output and process termination on cancellation. The initial stdin-EOF regression exposed a hanging fixture; fixed by dropping stdin after writing, with bounded tests added. No fixture process remains.
- Local installed tesseract also recognized HELLO 123 from a generated PNG through the actual stdin/stdout command. This is an OCR engine smoke test, not a live external-provider test. Claude was not used.

## Regressions
- Full local core library suite: ChatState 465, memory 302, Pager 7152 (10 ignored), pager-minimal 86, Sampler 218, sampling-types 266, Shell 3759 (3 ignored), Workflow 65. Total 12313 passed, 0 failed, 13 existing ignored. Log: /tmp/grow-r27-full-core.log.
- Tools persistence regression for canonical provider/model markers: 13 passed, 0 failed. Log: /tmp/grow-r27-resource-tests.log.
- Final targeted image regression after consistent user-facing error mapping: 100 passed, 0 failed (46.16 seconds). Log: /tmp/grow-r27-final-image-tests.log.
- Final Linux CI and release execution are recorded separately; no publication inferred from local tests.
