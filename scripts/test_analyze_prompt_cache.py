import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest

import analyze_prompt_cache as tool


class PromptCacheAnalyzerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / "artifacts" / "sampling").mkdir(parents=True)

    def tearDown(self):
        self.tmp.cleanup()

    def write_snapshot(self, bodies, *, route="https://api.example/v1/responses", usage=True):
        events = []
        for index, (identity, body, projection) in enumerate(bodies, 1):
            rid, attempt = identity.rsplit(":", 1)
            raw = json.dumps(body, separators=(",", ":")).encode()
            chunks = []
            for offset in range(0, len(raw), tool.CHUNK_BYTES):
                chunk = raw[offset:offset + tool.CHUNK_BYTES]
                digest = tool.blake3(chunk)
                (self.root / "artifacts" / "sampling" / f"{digest}.bin").write_bytes(chunk)
                chunks.append({"blake3": digest, "bytes": len(chunk)})
            events.append({"version": 1, "seq": 2 * index - 1, "at_ms": 2 * index - 1,
                           "type": "observation", "event": {"scope": "sampling_evidence", "name": "request",
                           "turn": None, "step": None, "data": {"owner": {"request_id": rid, "source_projection": projection},
                           "kind": "request", "metadata": {"backend": "openai", "route": route, "attempt_number": int(attempt)},
                           "bytes": len(raw), "chunks": chunks}}})
            if usage:
                events.append({"version": 1, "seq": 2 * index, "at_ms": 2 * index,
                               "type": "request", "event": {"state": "completed", "id": rid, "attempt": int(attempt),
                               "duration_ms": 4, "usage": {"input_tokens": 12, "output_tokens": 3,
                               "cache_read_tokens": 0}, "response_message_count": 1}})
        (self.root / "timeline.jsonl").write_text("".join(json.dumps(e) + "\n" for e in events))

    @staticmethod
    def projection(seq, epoch="e1"):
        return {"timeline_id": "session-1", "source_seq": seq, "surface_ids": ["s1"],
                "image_budget": {"evicted_parts": []}, "continuation_epoch": epoch,
                "native_spans": []}

    def test_blake3_vectors_and_protocol_differences(self):
        self.assertEqual(tool.blake3(b""), "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262")
        self.assertEqual(tool.blake3(b"abc"), "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85")
        pairs = [
            ("chat_completions", {"model": "m", "messages": [{"role": "user", "content": "SECRET"}], "tools": [{"name": "a"}]},
             {"model": "m", "messages": [{"role": "user", "content": "SECRET"}], "tools": [{"name": "b"}]}),
            ("responses", {"model": "m", "input": [{"type": "message", "content": "SECRET"}], "reasoning": {"effort": "low"}},
             {"model": "m", "input": [{"type": "message", "content": "SECRET"}], "reasoning": {"effort": "high"}}),
            ("messages", {"model": "m", "max_tokens": 20, "system": "SECRET", "messages": []},
             {"model": "m", "max_tokens": 30, "system": "SECRET", "messages": []}),
        ]
        for i, (protocol, base, request) in enumerate(pairs):
            with self.subTest(protocol=protocol):
                self.write_snapshot([(f"r{i}:1", base, self.projection(1)), (f"r{i+1}:1", request, self.projection(2, "e2"))])
                report = tool._report(self.root, f"r{i}:1", f"r{i+1}:1")
                self.assertEqual(report["comparison"]["protocol"], {"baseline": protocol, "request": protocol, "same": True})
                self.assertEqual(report["comparability"]["status"], "not_comparable")
                self.assertTrue(report["comparability"]["same_visible_endpoint"])
                self.assertFalse(report["comparability"]["same_confirmed_route"])
                self.assertTrue(any(section["changed"] for section in report["comparison"]["sections"].values()))
                rendered = json.dumps(report)
                self.assertNotIn("SECRET", rendered)
                self.assertTrue(report["source_projection"]["continuation_epoch"]["changed"])

    def test_projection_image_selection_and_native_spans_are_compared(self):
        left = self.projection(1)
        right = self.projection(2)
        right["image_budget"]["evicted_parts"] = ["surface-part-4"]
        right["native_spans"] = [[2, 5]]
        diff = tool._projection_diff(left, right)
        self.assertTrue(diff["image_budget"]["changed"])
        self.assertTrue(diff["native_spans"]["changed"])
        self.assertFalse(diff["surface_ids"]["changed"])

    def test_reordered_tools_stay_changed_and_history_append_is_structural(self):
        before = {"model": "m", "messages": [{"role": "user", "content": "secret"}], "tools": [{"name": "a"}, {"name": "b"}]}
        after = {"model": "m", "messages": [{"role": "user", "content": "secret"}, {"role": "assistant", "content": "hidden"}], "tools": [{"name": "b"}, {"name": "a"}]}
        delta = tool._compare_body(before, after)
        self.assertTrue(delta["sections"]["tools"]["changed"])
        self.assertTrue(delta["sections"]["history"]["append_only"])

    def test_cache_hints_and_route_or_branch_changes_are_reported(self):
        left = {"model": "m", "messages": [{"role": "user", "content": [
            {"type": "text", "text": "private", "cache_control": {"type": "ephemeral"}}]}]}
        right = {"model": "m", "messages": [{"role": "user", "content": [
            {"type": "text", "text": "private", "cache_control": {"type": "persistent"}}]}]}
        delta = tool._compare_body(left, right)
        self.assertTrue(delta["sections"]["cache_hints"]["changed"])
        self.write_snapshot([("a:1", left, self.projection(1)), ("b:1", right, self.projection(2))])
        timeline = self.root / "timeline.jsonl"
        rows = [json.loads(line) for line in timeline.read_text().splitlines()]
        for row in rows:
            if row.get("type") == "observation" and row["event"].get("data", {}).get("owner", {}).get("request_id") == "b":
                row["event"]["data"]["metadata"]["route"] = "https://different.example/v1/messages"
        timeline.write_text("".join(json.dumps(row) + "\n" for row in rows))
        report = tool._report(self.root, "a:1", "b:1")
        self.assertEqual(report["comparability"]["status"], "not_comparable")
        self.assertNotEqual(report["baseline"]["route"]["digest"], report["request"]["route"]["digest"])

    def test_different_timeline_branch_is_not_comparable(self):
        body = {"model": "m", "input": []}
        self.write_snapshot([("a:1", body, self.projection(1)), ("b:1", body, self.projection(2))])
        timeline = self.root / "timeline.jsonl"
        rows = [json.loads(line) for line in timeline.read_text().splitlines()]
        for row in rows:
            if row.get("type") == "observation" and row["event"].get("data", {}).get("owner", {}).get("request_id") == "b":
                row["event"]["data"]["owner"]["source_projection"]["timeline_id"] = "other-branch"
        timeline.write_text("".join(json.dumps(row) + "\n" for row in rows))
        report = tool._report(self.root, "a:1", "b:1")
        self.assertFalse(report["comparability"]["same_branch"])
        self.assertEqual(report["comparability"]["status"], "not_comparable")

    def test_retry_attempts_are_explicit_and_sideband_needs_lineage(self):
        body = {"model": "m", "input": []}
        self.write_snapshot([("retry:1", body, self.projection(1)), ("retry:2", body, self.projection(2))])
        report = tool._report(self.root, "retry:1", "retry:2")
        self.assertEqual(report["baseline"]["identity"], "retry:1")
        self.assertEqual(report["request"]["identity"], "retry:2")
        timeline = self.root / "timeline.jsonl"
        rows = [json.loads(line) for line in timeline.read_text().splitlines()]
        for row in rows:
            if row.get("type") == "observation":
                data = row["event"]["data"]
                data["owner"] = {"sideband_id": "sideband-1"}
                data["metadata"].pop("attempt_number")
                data["metadata"]["sideband_attempt"] = 1 if row["seq"] == 1 else 2
        timeline.write_text("".join(json.dumps(row) + "\n" for row in rows))
        sideband = tool._report(self.root, "sideband-1:1", "sideband-1:2")
        self.assertEqual(sideband["comparability"]["status"], "not_comparable")
        self.assertEqual(sideband["baseline"]["usage"]["status"], "unavailable")

    def test_unknown_route_or_source_lineage_is_not_comparable(self):
        base = {"model": "m", "input": []}
        self.write_snapshot([("a:1", base, None), ("b:1", base, None)], route="")
        report = tool._report(self.root, "a:1", "b:1")
        self.assertEqual(report["comparability"]["status"], "not_comparable")

    def test_missing_digest_and_symlink_references_fail_read_only(self):
        body = {"model": "m", "input": []}
        self.write_snapshot([("a:1", body, self.projection(1)), ("b:1", body, self.projection(2))])
        timeline = self.root / "timeline.jsonl"
        original = timeline.read_bytes()
        artifact = next((self.root / "artifacts" / "sampling").glob("*.bin"))
        artifact.write_bytes(b"tampered")
        with self.assertRaisesRegex(tool.EvidenceError, "content mismatch"):
            tool._report(self.root, "a:1", "b:1")
        self.assertEqual(timeline.read_bytes(), original)

    def test_escaping_symlink_is_rejected(self):
        body = {"model": "m", "input": []}
        self.write_snapshot([("a:1", body, self.projection(1)), ("b:1", body, self.projection(2))])
        outside = self.root.parent / (self.root.name + "-outside")
        outside.mkdir(exist_ok=True)
        sampling = self.root / "artifacts" / "sampling"
        moved = self.root / "artifacts" / "sampling-saved"
        sampling.rename(moved)
        sampling.symlink_to(outside, target_is_directory=True)
        try:
            with self.assertRaisesRegex(tool.EvidenceError, "symlink"):
                tool._report(self.root, "a:1", "b:1")
        finally:
            sampling.unlink()
            moved.rename(sampling)
            outside.rmdir()

    def test_cli_outputs_deterministic_redacted_json_and_text(self):
        body = {"model": "m", "input": [{"text": "private prompt body"}]}
        self.write_snapshot([("a:1", body, self.projection(1)), ("b:1", body, self.projection(2))])
        argv = ["--session-dir", str(self.root), "--baseline", "a:1", "--request", "b:1", "--format", "json"]
        first, second = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(first):
            self.assertEqual(tool.main(argv), 0)
        with contextlib.redirect_stdout(second):
            self.assertEqual(tool.main(argv), 0)
        self.assertEqual(first.getvalue(), second.getvalue())
        self.assertNotIn("private prompt body", first.getvalue())
        self.assertIn("cache_read_tokens", first.getvalue())
        text_out = io.StringIO()
        with contextlib.redirect_stdout(text_out):
            self.assertEqual(tool.main(argv[:-1] + ["text"]), 0)
        self.assertIn("Comparable:", text_out.getvalue())
        self.assertNotIn("private prompt body", text_out.getvalue())

    def test_declared_oversize_body_is_rejected_before_chunk_reads(self):
        body = {"model": "m", "input": []}
        self.write_snapshot([("a:1", body, self.projection(1)), ("b:1", body, self.projection(2))])
        timeline = self.root / "timeline.jsonl"
        rows = [json.loads(line) for line in timeline.read_text().splitlines()]
        for row in rows:
            if row.get("type") == "observation":
                row["event"]["data"]["bytes"] = tool.MAX_BODY_BYTES + 1
                break
        timeline.write_text("".join(json.dumps(row) + "\n" for row in rows))
        with self.assertRaisesRegex(tool.EvidenceError, "invalid body size"):
            tool._report(self.root, "a:1", "b:1")

    def test_usage_presence_distinguishes_zero_and_missing(self):
        self.assertEqual(tool._usage({"usage": {"input_tokens": 0, "cache_read_tokens": 0}})["field_presence"]["cache_read_tokens"], True)
        self.assertEqual(tool._usage({"usage": {"input_tokens": 0}})["field_presence"]["cache_read_tokens"], False)
        self.assertEqual(tool._usage({"usage": {"input_tokens": 0, "cache_read_tokens": None}})["field_presence"]["cache_read_tokens"], False)
        self.assertEqual(tool._usage(None)["status"], "unavailable")
        known = tool._usage({"usage": {"input_tokens": 100, "output_tokens": 5,
                                        "cache_read_tokens": 80}})
        unknown = tool._usage({"usage": {"input_tokens": 900, "output_tokens": 5}})
        self.assertEqual(known["cache_coverage"]["read_known_input_tokens"] + unknown["cache_coverage"]["read_known_input_tokens"], 100)
        self.assertEqual(known["cache_coverage"]["read_unknown_calls"] + unknown["cache_coverage"]["read_unknown_calls"], 1)
        self.assertEqual(known["cache_coverage"]["write_unknown_calls"], 1)
        self.assertEqual(known["source"], "timeline_settlement")

    def test_sideband_settlement_joins_attempt_and_preserves_unknown_cache(self):
        body = {"model": "m", "input": []}
        self.write_snapshot([("a:1", body, self.projection(1)), ("b:1", body, self.projection(2))])
        timeline = self.root / "timeline.jsonl"
        rows = [json.loads(line) for line in timeline.read_text().splitlines()]
        for row in rows:
            if row["type"] == "observation":
                data = row["event"]["data"]
                data["owner"] = {"sideband_id": "sb-1"}
                data["metadata"]["sideband_attempt"] = 1 if row["seq"] == 1 else 2
                data["metadata"].pop("attempt_number")
        rows.append({"version": 1, "seq": 5, "at_ms": 5, "type": "observation",
                     "event": {"scope": "sampling_usage", "name": "aux_attempt_settled", "data": {
                         "key": {"sideband_id": "sb-1", "attempt_no": 2}, "usage": {
                             "prompt_tokens": 100, "completion_tokens": 5, "total_tokens": 105,
                             "cached_prompt_tokens": 0, "cache_read_known": True,
                             "cache_creation_prompt_tokens": 0, "cache_write_known": False}}}})
        timeline.write_text("".join(json.dumps(row) + "\n" for row in rows))
        report = tool._report(self.root, "sb-1:1", "sb-1:2")
        self.assertEqual(report["baseline"]["usage"]["status"], "unavailable")
        settled = report["request"]["usage"]
        self.assertEqual(settled["source"], "auxiliary_settlement")
        self.assertEqual(settled["fields"], {"input_tokens": 100, "output_tokens": 5,
                                             "cache_read_tokens": 0})
        self.assertFalse(settled["field_presence"]["cache_write_tokens"])
        self.assertEqual(settled["cache_coverage"]["read_known_input_tokens"], 100)
        self.assertEqual(settled["cache_coverage"]["write_unknown_calls"], 1)

    def test_raw_provider_usage_reports_only_allowlisted_numeric_fields(self):
        response = b'data: {"type":"response.completed","response":{"usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":0,"cache_write_tokens":70},"secret":"DO-NOT-PRINT"}}}\n\n'
        result = tool._raw_usage(response)
        self.assertEqual(result["frames"]["response.completed"]["input_tokens_details.cache_write_tokens"], 70)
        self.assertEqual(result["frames"]["response.completed"]["input_tokens_details.cached_tokens"], 0)
        self.assertNotIn("DO-NOT-PRINT", json.dumps(result))
        self.assertEqual(tool._cache_cross_check(result, {"fields": {"cache_read_tokens": 0,
            "cache_write_tokens": 70}}), {"read": "matches", "write": "matches"})
        self.assertEqual(tool._cache_cross_check(result, {"fields": {"cache_read_tokens": 1,
            "cache_write_tokens": 70}})["read"], "differs")
        messages = b'data: {"type":"message_start","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":2}}}\n\ndata: {"type":"message_delta","usage":{"output_tokens":3,"cache_creation_input_tokens":0}}\n\n'
        result = tool._raw_usage(messages)
        self.assertEqual(result["frames"]["message_start"]["cache_read_input_tokens"], 2)
        self.assertEqual(result["frames"]["message_delta"]["cache_creation_input_tokens"], 0)

    def test_selected_raw_response_is_joined_to_settled_usage(self):
        self.write_snapshot([("a:1", {"model": "m", "input": []}, self.projection(1)),
                             ("b:1", {"model": "m", "input": []}, self.projection(2))])
        raw = b'data: {"type":"response.completed","response":{"usage":{"input_tokens":12,"input_tokens_details":{"cached_tokens":0}}}}\n\n'
        digest = tool.blake3(raw)
        (self.root / "artifacts" / "sampling" / f"{digest}.bin").write_bytes(raw)
        event = {"version": 1, "seq": 5, "at_ms": 5, "type": "observation",
                 "event": {"scope": "sampling_evidence", "name": "response", "turn": None,
                           "step": None, "data": {"owner": {"request_id": "b"},
                           "kind": "response", "metadata": {"attempt_number": 1},
                           "bytes": len(raw), "chunks": [{"blake3": digest, "bytes": len(raw)}]}}}
        with (self.root / "timeline.jsonl").open("a") as handle:
            handle.write(json.dumps(event) + "\n")
        report = tool._report(self.root, "a:1", "b:1")
        self.assertEqual(report["request"]["raw_usage"]["status"], "known")
        self.assertEqual(report["request"]["cache_cross_check"]["read"], "matches")
        self.assertEqual(report["baseline"]["raw_usage"]["status"], "unavailable")


if __name__ == "__main__":
    unittest.main()
