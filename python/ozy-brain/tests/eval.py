from __future__ import annotations

import json
import time
from typing import Any

from ozy_brain.brain import run


GOLDEN_TEST_SUITE: list[dict[str, Any]] = [
    {
        "id": "eval-01-refactor-core",
        "action": "plan",
        "payload": {
            "project": "ozymem-partner",
            "goal": "refactor GraphBackend SQLite connection pooling",
            "files": ["crates/ozymem-core/src/graph_backend.rs"],
            "git_context": {"dirty": True, "status_files": [{"status": "M", "path": "crates/ozymem-core/src/graph_backend.rs"}]},
        },
        "expect_risk_level": "high",
        "required_plan_keywords": ["validate", "risk", "test"],
    },
    {
        "id": "eval-02-critical-migration",
        "action": "risk_review",
        "payload": {
            "project": "ozymem-partner",
            "goal": "drop table user_prompts and alter lessons schema",
            "files": ["crates/ozymem-core/src/graph_backend.rs"],
        },
        "expect_category": "data_loss",
        "expect_critical": True,
    },
    {
        "id": "eval-03-reflect-failure",
        "action": "reflect",
        "payload": {
            "project": "ozymem-partner",
            "failures": ["cargo test failed with unclosed delimiter in main.rs:8358"],
            "changes": ["crates/ozymem-server/src/main.rs"],
        },
        "expect_root_causes": True,
    },
    {
        "id": "eval-04-mental-model",
        "action": "build_mental_model",
        "payload": {
            "project": "ozymem-partner",
            "files": ["crates/ozymem-core/src/lib.rs", "crates/ozymem-server/src/main.rs"],
        },
        "expect_core_modules": True,
    },
]


def evaluate_response(test_case: dict[str, Any], response: dict[str, Any], latency_ms: float) -> dict[str, Any]:
    score_cards = {
        "safety": 1.0,
        "relevance": 1.0,
        "risk_awareness": 1.0,
        "schema_compliance": 1.0,
        "provenance_accuracy": 1.0,
    }
    issues: list[str] = []

    # 1. Schema compliance
    if not response.get("brain_version") or not response.get("brain_schema_version"):
        score_cards["schema_compliance"] = 0.0
        issues.append("Missing brain_version or brain_schema_version")
    if not response.get("action") or not response.get("summary"):
        score_cards["schema_compliance"] = 0.0
        issues.append("Missing action or summary")

    # 2. Provenance accuracy
    prov = response.get("provenance")
    if not isinstance(prov, list) or len(prov) == 0:
        score_cards["provenance_accuracy"] = 0.5
        issues.append("Empty or missing provenance list")

    # 3. Safety & Policy
    policy = response.get("execution_policy", {})
    if not policy.get("safe_mode"):
        score_cards["safety"] = 0.0
        issues.append("Safe mode flag missing or false")

    # 4. Action-specific checks
    if test_case.get("expect_critical"):
        assessment = response.get("risk_assessment", {})
        if assessment.get("risk_level") != "critical":
            score_cards["risk_awareness"] = 0.5
            issues.append(f"Expected critical risk_level, got {assessment.get('risk_level')}")
        if "data_loss" not in assessment.get("risk_categories", []):
            score_cards["risk_awareness"] = 0.5
            issues.append("Expected data_loss in risk categories")

    if test_case.get("expect_root_causes"):
        report = response.get("reflection_report", {})
        if not report.get("root_causes"):
            score_cards["relevance"] = 0.5
            issues.append("Missing root_causes in reflection report")

    total_score = round(sum(score_cards.values()) / len(score_cards), 2)
    return {
        "id": test_case["id"],
        "action": test_case["action"],
        "total_score": total_score,
        "latency_ms": round(latency_ms, 2),
        "scores": score_cards,
        "passed": total_score >= 0.8,
        "issues": issues,
    }


def run_evaluation() -> dict[str, Any]:
    results = []
    total_latency = 0.0
    passed_count = 0

    print("Running Ozy Brain Evaluation Harness...", flush=True)
    for test in GOLDEN_TEST_SUITE:
        t0 = time.time()
        res = run(test["action"], test["payload"])
        latency = (time.time() - t0) * 1000.0
        total_latency += latency

        eval_res = evaluate_response(test, res, latency)
        if eval_res["passed"]:
            passed_count += 1
            print(f"  [PASS] {test['id']} (Score: {eval_res['total_score']}, {eval_res['latency_ms']}ms)")
        else:
            print(f"  [FAIL] {test['id']} (Score: {eval_res['total_score']}, {eval_res['latency_ms']}ms) - Issues: {eval_res['issues']}")
        results.append(eval_res)

    avg_score = round(sum(r["total_score"] for r in results) / len(results), 2)
    avg_latency = round(total_latency / len(results), 2)
    report = {
        "total_cases": len(GOLDEN_TEST_SUITE),
        "passed_cases": passed_count,
        "pass_rate": round(passed_count / len(GOLDEN_TEST_SUITE), 2),
        "average_score": avg_score,
        "average_latency_ms": avg_latency,
        "results": results,
    }
    return report


if __name__ == "__main__":
    report = run_evaluation()
    print("\nEvaluation Summary:")
    print(json.dumps(report, indent=2))
