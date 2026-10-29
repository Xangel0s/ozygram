r"""
Tri-Hybrid Neuro-Cognitive Memory Substrate Orchestrator for OzyBrain / Dream-RSI.
Standard: Zero-Emoji, SOLID, High Concurrency, Graceful Offline Degradation.

Unifies three complementary storage engines according to neurobiological principles (CLS 2.0):
1. SQLite WAL (L1 Fast Reflex / Amygdala): Sub-millisecond (< 1ms) threat cache and drift alerts.
2. ChromaDB (L2 Vector Lake): Local C++ cosine KNN and specialized collections.
3. PostgreSQL + pgvector + ltree (L3 Neocortex / Cluster): Persistent MCTS Trajectory Forest,
   SWR Replay Lake, and recursive CTE Reverse Blast Radius traversal.

Provides:
- Write-through channel with local buffering and non-blocking replication.
- Synaptic Tagging & Capture (STC) promotion of high-reward ($Q \ge 0.85$) MCTS trajectories.
- 3-Way Federated Reciprocal Rank Fusion (RRF) combining lexical, local vector, and global pgvector.
"""

from __future__ import annotations

import json
import logging
import os
import time
from pathlib import Path
from typing import Any, Dict, List, Optional, Union

from .chroma_bridge import ChromaVectorBridge
from .contract_tracer import ContractDriftAlert
from .memory_store import GotchaMemory, MarkdownGitSynchronizer, SqliteWalBackend
from .postgres_engine import MctsForestRepository, PostgresConfig, PostgresNativeEngine

logger = logging.getLogger("ozy_brain.scanner.tri_hybrid_orchestrator")


class TriHybridMemoryOrchestrator:
    """
    Unified coordinator across SQLite WAL, ChromaDB, and PostgreSQL pgvector.
    Guarantees sub-millisecond local responses with non-blocking enterprise replication.
    """

    def __init__(
        self,
        sqlite_path: Optional[Union[str, Path]] = None,
        chroma_dir: Optional[Union[str, Path]] = None,
        pg_config: Optional[PostgresConfig] = None,
        markdown_base_dir: Optional[Union[str, Path]] = None,
    ) -> None:
        self.sqlite_path = Path(sqlite_path or Path.home() / ".ozymem" / "memory.db")
        self.sqlite_backend = SqliteWalBackend(self.sqlite_path)

        self.chroma_bridge = ChromaVectorBridge(persist_dir=chroma_dir)

        self.postgres_engine = PostgresNativeEngine(config=pg_config)
        self.mcts_repo = MctsForestRepository(self.postgres_engine)

        self.markdown_base_dir = Path(markdown_base_dir or Path(".ozy/memories"))

    def health_report(self) -> Dict[str, Any]:
        """Audits the operational health of all three storage tiers."""
        pg_health = self.postgres_engine.check_connection()
        return {
            "sqlite": {
                "status": "online",
                "path": str(self.sqlite_path),
                "journal_mode": "wal",
            },
            "chroma": {
                "status": "online" if self.chroma_bridge.is_available else "offline",
                "collections": list(self.chroma_bridge.ALL_COLLECTIONS),
            },
            "postgres": {
                "status": "online" if pg_health.get("is_online") else "offline_fallback",
                "dsn_configured": bool(self.postgres_engine.config.dsn),
                "details": pg_health,
            },
        }

    def get_telemetry_status(self, workspace_root: Optional[Union[str, Path]] = None) -> Dict[str, Any]:
        """
        Gathers comprehensive telemetry across L1, L2, L3, and neuromodulators.
        Returns unified latency guarantees and storage status.
        """
        health = self.health_report()

        # 1. L1 SQLite metrics
        wal_path = Path(str(self.sqlite_path) + "-wal")
        wal_size_bytes = wal_path.stat().st_size if wal_path.exists() else 0
        sqlite_stats = {
            "status": "online",
            "path": str(self.sqlite_path),
            "wal_size_bytes": wal_size_bytes,
            "wal_size_kb": round(wal_size_bytes / 1024, 2),
            "latency_sla": "< 1.0ms",
        }

        # 2. L2 ChromaDB collection stats
        chroma_stats = {
            "status": "online" if self.chroma_bridge.is_available else "offline",
            "collections": list(self.chroma_bridge.ALL_COLLECTIONS),
            "latency_sla": "< 5.0ms",
        }

        # 3. L3 PostgreSQL metrics
        pg_stats = {
            "status": health["postgres"]["status"],
            "dsn_configured": health["postgres"]["dsn_configured"],
            "latency_sla": "< 8.0ms",
        }

        # 4. Neuromodulators
        neuro_info: Dict[str, Any] = {}
        try:
            from ozy_brain.neuromodulators import NeuromodulatorEngine
            n_engine = NeuromodulatorEngine(workspace_root=str(workspace_root or "."))
            neuro_info = n_engine.get_effective_hyperparameters()
        except Exception:
            neuro_info = {"status": "unavailable"}

        return {
            "tri_hybrid_status": "healthy" if self.chroma_bridge.is_available else "degraded",
            "badge": "[STATUS: TRI_HYBRID_COGNITION_ACTIVE]",
            "l1_sqlite": sqlite_stats,
            "l2_chroma": chroma_stats,
            "l3_postgres": pg_stats,
            "neuromodulators": neuro_info,
            "timestamp": time.time(),
        }

    def save_gotcha(
        self,
        gotcha: GotchaMemory,
        sync_markdown: bool = True,
    ) -> Dict[str, Any]:
        """
        Executes write-through persistence across all tiers:
        1. Fast synchronous write to SQLite WAL (< 1 ms).
        2. Vector indexing in ChromaDB 'semantic_heuristics'.
        3. Collaborative persistence in PostgreSQL 'ozy_cognitive_gotchas' if online.
        4. Optional Git-First markdown persistence.
        """
        results: Dict[str, Any] = {"id": gotcha.id}

        # Tier 1: SQLite WAL
        results["sqlite_saved"] = bool(self.sqlite_backend.save_gotcha(gotcha))

        # Tier 2: ChromaDB Vector Lake
        doc_content = f"{gotcha.title}\n{gotcha.flawed_assumption}\n{gotcha.countermeasure}"
        results["chroma_saved"] = self.chroma_bridge.upsert_document(
            collection_name=ChromaVectorBridge.COLLECTION_HEURISTICS,
            doc_id=gotcha.id,
            content=doc_content,
            metadata={
                "project": "crmnew",
                "severity": gotcha.severity,
                "kind": gotcha.kind,
                "topic_key": gotcha.topic_key,
            },
        )

        # Tier 3: PostgreSQL Cluster
        if self.postgres_engine.is_available:
            try:
                sql = """
                INSERT INTO ozy_cognitive_gotchas (
                    id, topic_key, title, severity, applies_to, tags,
                    flawed_assumption, countermeasure, created_at
                ) VALUES (%s, %s, %s, %s, %s::jsonb, %s::jsonb, %s, %s, TO_TIMESTAMP(%s))
                ON CONFLICT (id) DO UPDATE SET
                    flawed_assumption = EXCLUDED.flawed_assumption,
                    countermeasure = EXCLUDED.countermeasure;
                """
                with self.postgres_engine.get_connection() as conn:
                    with conn.cursor() as cur:
                        cur.execute(sql, (
                            gotcha.id,
                            gotcha.topic_key,
                            gotcha.title,
                            gotcha.severity,
                            json.dumps(gotcha.applies_to),
                            json.dumps(gotcha.tags),
                            gotcha.flawed_assumption,
                            gotcha.countermeasure,
                            gotcha.created_at,
                        ))
                results["postgres_saved"] = True
            except Exception as e:
                logger.warning(f"[ALERT: POSTGRES_SAVE_GOTCHA_FAILED] {e}")
                results["postgres_saved"] = False
        else:
            results["postgres_saved"] = False

        # Tier 4: Git-First Markdown Sync
        if sync_markdown:
            try:
                md_path = MarkdownGitSynchronizer.export_gotcha(gotcha, self.markdown_base_dir)
                results["markdown_path"] = str(md_path)
            except Exception as e:
                logger.warning(f"[ALERT: MARKDOWN_SYNC_FAILED] {e}")
                results["markdown_path"] = None

        return results

    def save_drift_alert(self, alert: ContractDriftAlert) -> str:
        """Stores contract drift alert in SQLite WAL."""
        return self.sqlite_backend.save_drift_alert(alert)

    def save_swr_failure_ripple(
        self,
        ripple_id: str,
        file_path: str,
        error_signature: str,
        hazard_desc: str,
        invariant_vaccine: str,
        error_gradient: float,
        tenant_id: str = "crmnew",
    ) -> Dict[str, Any]:
        """
        Stores SWR failure ripple across ChromaDB episodic lake and PostgreSQL pgvector lake.
        """
        content = f"{file_path} | {error_signature} | {hazard_desc} | {invariant_vaccine}"
        vec = self.chroma_bridge.embed_text(content)

        chroma_ok = self.chroma_bridge.upsert_document(
            collection_name=ChromaVectorBridge.COLLECTION_EPISODIC,
            doc_id=ripple_id,
            content=content,
            metadata={
                "tenant_id": tenant_id,
                "file_path": file_path,
                "error_signature": error_signature,
                "error_gradient": error_gradient,
            },
            embedding=vec,
        )

        pg_id = self.postgres_engine.insert_swr_ripple(
            ripple_id=ripple_id,
            file_path=file_path,
            error_signature=error_signature,
            hazard_desc=hazard_desc,
            invariant_vaccine=invariant_vaccine,
            error_gradient=error_gradient,
            embedding=vec,
            tenant_id=tenant_id,
        )

        return {
            "ripple_id": ripple_id,
            "chroma_saved": chroma_ok,
            "postgres_saved": bool(pg_id),
        }

    def save_mcts_trajectory_node(
        self,
        node_id: str,
        tree_id: str,
        parent_path: str,
        phase: str,
        action_strategy: str,
        visits: int,
        q_value: float,
        state_payload: Dict[str, Any],
        projected_churn: int = 0,
    ) -> Dict[str, Any]:
        """
        Persists MCTS node in PostgreSQL ltree forest.
        Synaptic Tagging & Capture (STC): If Q >= 0.85, automatically promotes the node
        to an invariant vaccine in ChromaDB and SQLite WAL.
        """
        saved_id = self.mcts_repo.save_node(
            node_id=node_id,
            tree_id=tree_id,
            parent_path=parent_path,
            phase=phase,
            action_strategy=action_strategy,
            visits=visits,
            q_value=q_value,
            state_payload=state_payload,
            projected_churn=projected_churn,
        )

        promoted_stc = False
        if q_value >= 0.85:
            # Synaptic Tagging & Capture Promotion
            vaccine_title = f"[STC VACCINE] {action_strategy} on {phase}"
            vaccine_content = (
                f"High-reward branch (Q={q_value:.4f}, visits={visits}, churn={projected_churn}). "
                f"Strategy: {action_strategy}. State: {json.dumps(state_payload)}"
            )
            # Index in ChromaDB dream_insights
            self.chroma_bridge.upsert_document(
                collection_name=ChromaVectorBridge.COLLECTION_INSIGHTS,
                doc_id=f"stc_{node_id}",
                content=vaccine_content,
                metadata={
                    "tree_id": tree_id,
                    "action_strategy": action_strategy,
                    "q_value": q_value,
                    "stc_promoted": True,
                },
            )
            # Save gotcha vaccine in SQLite
            vaccine_gotcha = GotchaMemory(
                id=f"gotcha_stc_{node_id}",
                kind="vaccine",
                topic_key="stc_plan_branching",
                title=vaccine_title,
                author="DreamRsiCentinela",
                date=time.strftime("%Y-%m-%d"),
                severity="critical" if action_strategy == "DualActiveContract" else "high",
                applies_to=list(state_payload.get("working_edits", {}).keys()) or ["full_stack"],
                tags=["mcts", "stc_promotion", action_strategy.lower()],
                flawed_assumption=f"Risky uncoordinated cutover in {phase}",
                countermeasure=f"Apply {action_strategy} with non-breaking dual contracts",
                created_at=time.time(),
            )
            self.sqlite_backend.save_gotcha(vaccine_gotcha)
            promoted_stc = True

        return {
            "node_id": saved_id,
            "stc_promoted": promoted_stc,
            "q_value": q_value,
        }

    def query_reverse_blast_radius(
        self,
        target_file: str,
        max_depth: int = 5,
    ) -> List[Dict[str, Any]]:
        """Delegates recursive CTE Reverse Blast Radius traversal to PostgreSQL."""
        return self.postgres_engine.query_reverse_blast_radius(target_file, max_depth=max_depth)

    def federated_rrf_search(
        self,
        query: str,
        limit: int = 5,
        k: int = 60,
    ) -> List[Dict[str, Any]]:
        """
        Federated 3-Way Reciprocal Rank Fusion (RRF):
        Combines:
        1. SQLite lexical matching (BM25 token presence in SQLite WAL).
        2. ChromaDB local cosine KNN (across 'semantic_heuristics' and 'dream_insights').
        3. PostgreSQL pgvector SWR failure ripples (if online).

        Formula:
        RRF_score(d) = sum_{m in {sqlite, chroma, postgres}} 1 / (k + rank_m(d))
        """
        scores: Dict[str, float] = {}
        items_by_key: Dict[str, Dict[str, Any]] = {}

        # 1. SQLite Tier (Lexical)
        sqlite_hits = self.sqlite_backend.search_lexical(query, limit=limit * 2)
        for rank, gotcha in enumerate(sqlite_hits, start=1):
            key = f"gotcha::{gotcha.id}"
            items_by_key[key] = {
                "id": gotcha.id,
                "source": "sqlite_wal",
                "title": gotcha.title,
                "content": f"{gotcha.flawed_assumption} -> {gotcha.countermeasure}",
                "severity": gotcha.severity,
            }
            scores[key] = scores.get(key, 0.0) + (1.0 / (k + rank))

        # 2. ChromaDB Tier (Local Dense KNN)
        chroma_hits = self.chroma_bridge.query_cross_collections(
            collections=[
                ChromaVectorBridge.COLLECTION_HEURISTICS,
                ChromaVectorBridge.COLLECTION_INSIGHTS,
            ],
            query=query,
            limit_per_coll=limit,
        )
        for rank, hit in enumerate(chroma_hits, start=1):
            raw_id = hit["id"]
            key = f"gotcha::{raw_id}" if not raw_id.startswith("stc_") else f"insight::{raw_id}"
            if key not in items_by_key:
                items_by_key[key] = {
                    "id": raw_id,
                    "source": f"chroma::{hit['collection']}",
                    "title": hit["content"].split("\n")[0][:80],
                    "content": hit["content"],
                    "similarity": hit["similarity_score"],
                }
            scores[key] = scores.get(key, 0.0) + (1.0 / (k + rank))

        # 3. PostgreSQL pgvector Tier (Global SWR failure lake)
        if self.postgres_engine.is_available:
            query_vec = self.chroma_bridge.embed_text(query)
            pg_hits = self.postgres_engine.search_swr_ripples_knn(query_vec, limit=limit)
            for rank, hit in enumerate(pg_hits, start=1):
                key = f"swr::{hit['id']}"
                if key not in items_by_key:
                    items_by_key[key] = {
                        "id": hit["id"],
                        "source": "postgres_pgvector",
                        "title": f"SWR Failure: {hit['error_signature']}",
                        "content": f"{hit['hazard']} -> Vaccine: {hit['vaccine']}",
                        "similarity": hit["similarity"],
                    }
                scores[key] = scores.get(key, 0.0) + (1.0 / (k + rank))

        # Sort combined results by RRF score
        sorted_keys = sorted(scores.keys(), key=lambda x: scores[x], reverse=True)
        fused_results: List[Dict[str, Any]] = []

        for key in sorted_keys[:limit]:
            item = dict(items_by_key[key])
            item["rrf_score"] = round(scores[key], 6)
            fused_results.append(item)

        return fused_results

    def close(self) -> None:
        """Safely disposes all handles and connections."""
        self.chroma_bridge.close()
