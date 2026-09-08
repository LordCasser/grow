## Evidence
Both complete_clipboard_attachment_paste implementations try a file classifier then recover only source.text_to_insert_on_miss. The source's original text does not include URLs found by the asynchronous probe. Failed text reads can nevertheless have a successful file URL probe.
## Design
Add a small shared source method selecting non-empty URL text only when original text is absent/whitespace. Call it only after attachment FullMiss and no classified file result. Map target text insertion to the existing file completion result, so successfully recovered URL text has the same precedence over original text-read failure as successfully classified paths. Do not invoke this after probe failure/drop, persistence failure, target rejection, question-mode discard or stale Dashboard target checks.
## Limits
This does not retry attachment admission or decode the URLs. Text insertion retains normal widget sanitization/folding and queue rules. Non-empty source text, including already-inserted bracketed text, suppresses URL fallback.
