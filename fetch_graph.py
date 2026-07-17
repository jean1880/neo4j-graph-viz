#!/usr/bin/env python3
"""Pull the whole Neo4j graph into a static graph.json for the force-graph viewer.

Runs on this PC, reads NEO4J_PASSWORD from the environment (never hardcoded), and
writes {nodes, links} shaped for vasturiano's force-graph. No secrets touch the
browser — the frontend only ever loads the resulting JSON.
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

from neo4j import GraphDatabase

GRAPH_HOST = os.environ.get("NEO4J_HOST", "bolt://localhost:7687")
OUT = Path(__file__).with_name("graph.json")

# Labels that are structural wrappers rather than the meaningful type.
GENERIC = {"MH"}

# Property keys to omit from the detail overlay (embeddings, internal stamps).
SKIP_PROPS = {"embedding", "vector", "sync_id"}
MAX_VAL = 600  # truncate long string values so graph.json stays lean


def clean_props(props: dict) -> dict:
    """Stringify + truncate node properties for display in the overlay."""
    out: dict[str, str] = {}
    for key, val in props.items():
        if key in SKIP_PROPS:
            continue
        if isinstance(val, (list, dict)):
            val = json.dumps(val, ensure_ascii=False)
        text = str(val)
        if len(text) > MAX_VAL:
            text = text[:MAX_VAL] + "…"
        out[key] = text
    return out


def pick_label(labels: list[str]) -> tuple[str, str]:
    """Return (specific_label, group). Group folds MH:* into one domain."""
    specifics = [l for l in labels if l not in GENERIC]
    specific = specifics[0] if specifics else (labels[0] if labels else "Node")
    group = "MH" if "MH" in labels else "infra"
    return specific, group


def pick_name(props: dict) -> str:
    for key in ("name", "title", "id", "hostname", "canonical"):
        val = props.get(key)
        if isinstance(val, str) and val.strip():
            return val
    # fall back to first stringy prop
    for val in props.values():
        if isinstance(val, str) and val.strip():
            return val
    return "?"


def main() -> int:
    pw = os.environ.get("NEO4J_PASSWORD")
    if not pw:
        print("NEO4J_PASSWORD not set (source ~/.env first)", file=sys.stderr)
        return 1

    nodes: dict[str, dict] = {}
    links: list[dict] = []

    with GraphDatabase.driver(GRAPH_HOST, auth=("neo4j", pw)) as driver:
        with driver.session() as sess:
            for rec in sess.run(
                "MATCH (n) RETURN elementId(n) AS id, labels(n) AS labels, properties(n) AS props"
            ):
                label, group = pick_label(rec["labels"])
                nodes[rec["id"]] = {
                    "id": rec["id"],
                    "name": pick_name(rec["props"]),
                    "label": label,
                    "group": group,
                    "deg": 0,
                    "props": clean_props(rec["props"]),
                }
            for rec in sess.run(
                "MATCH (a)-[r]->(b) RETURN elementId(a) AS s, elementId(b) AS t, type(r) AS rel"
            ):
                s, t = rec["s"], rec["t"]
                if s in nodes and t in nodes:
                    links.append({"source": s, "target": t, "type": rec["rel"]})
                    nodes[s]["deg"] += 1
                    nodes[t]["deg"] += 1

    payload = {"nodes": list(nodes.values()), "links": links}
    # Atomic swap: the container serves graph.json while this runs hourly, so a
    # partial write must never be visible. os.replace is atomic on one filesystem.
    tmp = OUT.with_name(OUT.name + ".tmp")
    tmp.write_text(json.dumps(payload))
    tmp.replace(OUT)
    print(f"wrote {OUT} — {len(nodes)} nodes, {len(links)} links")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
