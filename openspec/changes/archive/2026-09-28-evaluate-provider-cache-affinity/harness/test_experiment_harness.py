import importlib.util
import json
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("experiment_harness", HERE / "experiment_harness.py")
harness = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(harness)
FIXTURE = json.loads((HERE / "fixtures" / "synthetic_workload.json").read_text(encoding="utf-8"))
BUDGETS = json.loads((HERE / "fixtures" / "mock_budgets.json").read_text(encoding="utf-8"))


class BudgetTests(unittest.TestCase):
    def test_mock_stops_at_route_request_output_and_cost_budgets(self):
        fixture = json.loads(json.dumps(FIXTURE))
        fixture["attempts"].append(dict(FIXTURE["attempts"][-1], scenario="static-prefix-repeat"))
        result = harness.run_mock(fixture, BUDGETS)
        self.assertEqual(result["network_requests_sent"], 0)
        self.assertEqual(result["route_totals"]["deepseek_direct_candidate"]["requests"], 2)
        self.assertEqual(result["stopped_by"]["deepseek_direct_candidate"], "request_limit")
        self.assertEqual(result["route_totals"]["openai_direct_candidate"]["output_tokens"], 512)
        self.assertEqual(result["route_totals"]["openai_direct_candidate"]["cost_usd"], "0.02")
        self.assertEqual(result["stopped_by"]["claude_messages_direct_candidate"], "request_limit")
        self.assertTrue(all(row["synthetic"] for row in result["evidence"]))
        self.assertTrue(all("".join(map(str, row.values())).find("api_key") < 0 for row in result["evidence"]))

    def test_mock_enforces_per_request_output_and_cost_ceiling(self):
        fixture = json.loads(json.dumps(FIXTURE))
        fixture["attempts"] = [dict(FIXTURE["attempts"][0], output_tokens=513)]
        result = harness.run_mock(fixture, {"deepseek_direct_candidate": BUDGETS["deepseek_direct_candidate"]})
        self.assertEqual(result["stopped_by"]["deepseek_direct_candidate"], "per_request_output_limit")
        self.assertEqual(result["evidence"], [])
        fixture["attempts"] = [dict(FIXTURE["attempts"][0], cost_usd="0.03")]
        result = harness.run_mock(fixture, {"deepseek_direct_candidate": BUDGETS["deepseek_direct_candidate"]})
        self.assertEqual(result["stopped_by"]["deepseek_direct_candidate"], "cost_limit")

    def test_mock_enforces_aggregate_output_token_ceiling(self):
        fixture = json.loads(json.dumps(FIXTURE))
        fixture["attempts"] = [FIXTURE["attempts"][0], FIXTURE["attempts"][1]]
        budget = {"max_requests": 3, "max_output_tokens_per_request": 256, "max_total_output_tokens": 256, "max_cost_usd": "1.00"}
        result = harness.run_mock(fixture, {"deepseek_direct_candidate": budget})
        self.assertEqual(result["route_totals"]["deepseek_direct_candidate"]["requests"], 1)
        self.assertEqual(result["stopped_by"]["deepseek_direct_candidate"], "total_output_limit")

    def test_all_three_prerequisite_artifacts_are_required(self):
        fixture = json.loads(json.dumps(FIXTURE))
        del fixture["prerequisites"]["offline_diagnostics"]
        with self.assertRaisesRegex(harness.HarnessError, "offline_diagnostics"):
            harness.run_mock(fixture, BUDGETS)

    def test_plan_requires_confirmed_route_and_never_sends(self):
        route = {"provider": "example", "model": "model-v1", "backend": "responses", "deployment": "test", "region": "test-region", "tenant_label": "isolated-test", "confirmed": True, "budget": BUDGETS["deepseek_direct_candidate"]}
        result = harness.build_plan(route, FIXTURE)
        self.assertEqual(result["network_requests_sent"], 0)
        self.assertEqual(result["execution_status"], "plan_only_sender_not_implemented")
        self.assertLessEqual(result["planned_requests"] * result["output_token_limit_per_request"], result["total_output_token_limit"])
        route["confirmed"] = False
        with self.assertRaisesRegex(harness.HarnessError, "confirmed=true"):
            harness.build_plan(route, FIXTURE)

    def test_plan_rejects_secret_fields_without_printing_values(self):
        route = {"provider": "example", "model": "model-v1", "backend": "responses", "deployment": "test", "region": "test-region", "tenant_label": "isolated-test", "confirmed": True, "budget": BUDGETS["deepseek_direct_candidate"], "api_key": "must-not-appear"}
        with self.assertRaisesRegex(harness.HarnessError, "secret-bearing"):
            harness.build_plan(route, FIXTURE)


if __name__ == "__main__":
    unittest.main()
