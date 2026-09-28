#!/usr/bin/env python3
"""Bounded offline harness for provider cache experiments.

This tool never sends network requests. Mock mode evaluates synthetic attempts;
plan mode validates an explicitly confirmed route and emits a bounded request
plan for a separately reviewed sender to execute.
"""
from __future__ import annotations

import argparse
import json
import sys
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any


REQUIRED_PREREQUISITES = {
    "image_projection": {"selected_images", "projection_domain"},
    "cache_usage_availability": {"full_input", "cache_read", "cache_write"},
    "offline_diagnostics": {"request_pair", "comparison_status"},
}
SECRET_KEYS = {"api_key", "env_key", "authorization", "headers", "query", "token", "password"}


class HarnessError(ValueError):
    pass


def _reject_secrets(value: Any, path: str = "config") -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            if str(key).lower() in SECRET_KEYS:
                raise HarnessError(f"{path} contains a secret-bearing field")
            _reject_secrets(child, f"{path}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            _reject_secrets(child, f"{path}[{index}]")


def _load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise HarnessError(f"cannot read JSON input {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise HarnessError(f"{path} must contain a JSON object")
    return value


def validate_fixture(fixture: dict[str, Any]) -> None:
    prerequisites = fixture.get("prerequisites")
    if not isinstance(prerequisites, dict):
        raise HarnessError("fixture must cite the three prerequisite artifacts")
    for artifact, fields in REQUIRED_PREREQUISITES.items():
        item = prerequisites.get(artifact)
        if not isinstance(item, dict) or not fields.issubset(item):
            raise HarnessError(f"fixture prerequisite {artifact} is missing required evidence")
    if fixture.get("data_classification") != "synthetic" or fixture.get("contains_real_session_data") is not False:
        raise HarnessError("mock fixture must explicitly contain synthetic data only")
    attempts = fixture.get("attempts")
    if not isinstance(attempts, list) or not attempts:
        raise HarnessError("fixture must contain at least one synthetic attempt")
    for index, attempt in enumerate(attempts):
        if not isinstance(attempt, dict):
            raise HarnessError(f"attempt {index} must be an object")
        for field in ("route_id", "scenario", "full_input_tokens", "output_tokens", "cost_usd", "usage"):
            if field not in attempt:
                raise HarnessError(f"attempt {index} is missing {field}")
        if not isinstance(attempt["usage"], dict):
            raise HarnessError(f"attempt {index} usage must preserve field availability")


def _decimal(value: Any, field: str) -> Decimal:
    try:
        result = Decimal(str(value))
    except (InvalidOperation, ValueError) as exc:
        raise HarnessError(f"{field} must be a decimal number") from exc
    if not result.is_finite() or result < 0:
        raise HarnessError(f"{field} must be finite and non-negative")
    return result


def _validate_budget(budget: dict[str, Any], prefix: str) -> dict[str, Any]:
    required = ("max_requests", "max_output_tokens_per_request", "max_total_output_tokens", "max_cost_usd")
    if any(field not in budget for field in required):
        raise HarnessError(f"{prefix} must define request, output-token, and cost limits")
    ints: dict[str, int] = {}
    for field in required[:3]:
        value = budget[field]
        if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
            raise HarnessError(f"{prefix}.{field} must be a positive integer")
        ints[field] = value
    cost = _decimal(budget["max_cost_usd"], f"{prefix}.max_cost_usd")
    if cost == 0:
        raise HarnessError(f"{prefix}.max_cost_usd must be greater than zero")
    return {**ints, "max_cost_usd": str(cost)}


def run_mock(fixture: dict[str, Any], budgets: dict[str, Any]) -> dict[str, Any]:
    validate_fixture(fixture)
    if not isinstance(budgets, dict) or not budgets:
        raise HarnessError("mock mode requires per-route budgets")
    clean_budgets = {route: _validate_budget(budget, f"budgets.{route}") for route, budget in budgets.items()}
    totals: dict[str, dict[str, Any]] = {}
    evidence: list[dict[str, Any]] = []
    stopped: dict[str, str] = {}
    for attempt in fixture["attempts"]:
        route = attempt["route_id"]
        if route not in clean_budgets:
            raise HarnessError(f"no budget configured for fixture route {route}")
        budget = clean_budgets[route]
        total = totals.setdefault(route, {"requests": 0, "output_tokens": 0, "cost_usd": Decimal(0)})
        if route in stopped:
            continue
        output = attempt["output_tokens"]
        cost = _decimal(attempt["cost_usd"], f"attempt.cost_usd[{route}]")
        if not isinstance(output, int) or isinstance(output, bool) or output < 0:
            raise HarnessError("attempt output_tokens must be a non-negative integer")
        reason = None
        if total["requests"] + 1 > budget["max_requests"]:
            reason = "request_limit"
        elif output > budget["max_output_tokens_per_request"]:
            reason = "per_request_output_limit"
        elif total["output_tokens"] + output > budget["max_total_output_tokens"]:
            reason = "total_output_limit"
        elif total["cost_usd"] + cost > Decimal(budget["max_cost_usd"]):
            reason = "cost_limit"
        if reason:
            stopped[route] = reason
            continue
        total["requests"] += 1
        total["output_tokens"] += output
        total["cost_usd"] += cost
        evidence.append({
            "route_id": route,
            "scenario": attempt["scenario"],
            "status": "mock_recorded",
            "full_input_tokens": attempt["full_input_tokens"],
            "output_tokens": output,
            "cost_usd": str(cost),
            "usage": attempt["usage"],
            "synthetic": True,
        })
    return {
        "mode": "mock",
        "network_requests_sent": 0,
        "prerequisites": fixture["prerequisites"],
        "route_totals": {route: {**values, "cost_usd": str(values["cost_usd"])} for route, values in totals.items()},
        "stopped_by": stopped,
        "evidence": evidence,
    }


def build_plan(route_config: dict[str, Any], fixture: dict[str, Any]) -> dict[str, Any]:
    _reject_secrets(route_config)
    validate_fixture(fixture)
    if route_config.get("confirmed") is not True:
        raise HarnessError("plan mode requires confirmed=true route configuration")
    fields = ("provider", "model", "backend", "deployment", "region", "tenant_label", "confirmed", "budget")
    missing = [field for field in fields if field not in route_config]
    if missing:
        raise HarnessError(f"confirmed route configuration missing: {', '.join(missing)}")
    if any(not isinstance(route_config[field], str) or not route_config[field].strip() for field in fields[:6]):
        raise HarnessError("route identity fields must be non-empty strings")
    budget = _validate_budget(route_config["budget"], "route.budget")
    scenarios = list(dict.fromkeys(row["scenario"] for row in fixture["attempts"]))
    output_bounded_requests = budget["max_total_output_tokens"] // budget["max_output_tokens_per_request"]
    planned_requests = min(budget["max_requests"], len(scenarios), output_bounded_requests)
    return {
        "mode": "plan",
        "network_requests_sent": 0,
        "route": {field: route_config[field] for field in fields[:6]},
        "budget": budget,
        "planned_requests": planned_requests,
        "scenarios": scenarios[:planned_requests],
        "output_token_limit_per_request": budget["max_output_tokens_per_request"],
        "total_output_token_limit": budget["max_total_output_tokens"],
        "cost_limit_usd": budget["max_cost_usd"],
        "execution_status": "plan_only_sender_not_implemented",
        "prerequisites": fixture["prerequisites"],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("mock", "plan"), default="mock", help="mock is offline; plan never sends requests")
    parser.add_argument("--fixture", type=Path, default=Path(__file__).parent / "fixtures" / "synthetic_workload.json")
    parser.add_argument("--budgets", type=Path, default=Path(__file__).parent / "fixtures" / "mock_budgets.json")
    parser.add_argument("--route-config", type=Path, help="explicit confirmed route JSON required in plan mode")
    args = parser.parse_args(argv)
    try:
        fixture = _load_json(args.fixture)
        if args.mode == "mock":
            result = run_mock(fixture, _load_json(args.budgets))
        else:
            if args.route_config is None:
                raise HarnessError("plan mode requires --route-config; no requests will be sent")
            result = build_plan(_load_json(args.route_config), fixture)
    except HarnessError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    json.dump(result, sys.stdout, ensure_ascii=False, indent=2)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
