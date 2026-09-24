# Design

Use the existing `run_request_task` HTTP fixture and evidence sink. Each injected response will be served once with a deterministic body, then the test will inspect the request, response and recovery-decision records in causal order. The raw byte assertion is intentionally made at the sink before Timeline artifact chunking; the existing artifact integrity and cold-load tests cover that separate storage boundary.

For the crash window, create a durable open attempt with no completed response admission, release the writer, then load through the ordinary recovery path. Assert that the recovered state reports uncertainty and has no accepted assistant response. A process killed before a durable ACK cannot yield a stronger historical claim; available records and the open state are the only facts to reconstruct.

Keep production code untouched unless a focused test demonstrates a defect. If one appears, update this change with a behavioral delta before fixing it.
