#!/usr/bin/env python3
"""HeartQ SquillaRouter V4 sidecar.

Reads one JSON object from stdin, writes one JSON object to stdout.

Input:
  {
    "message": "...",
    "prev_assistant_text": "...",
    "history_user_texts": ["..."],
    "routing_history": [{"route_class":"R1","difficulty":0.2,"margin":0.1}],
    "bundle_dir": "/optional/override",
    "use_aux_head": true
  }

Output:
  {
    "ok": true,
    "tier": "c1",
    "route_class": "R1",
    "confidence": 0.72,
    "difficulty": 0.41,
    "source": "v4_phase3",
    "thinking_mode": "T1",
    "prompt_policy": "P1",
    "probabilities": {"R0":0.1,"R1":0.7,"R2":0.15,"R3":0.05},
    "reasons": ["squilla_v4"]
  }
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from types import SimpleNamespace


def default_bundle_dir() -> Path:
    env = os.environ.get("HEARTQ_SQUILLA_BUNDLE")
    if env:
        return Path(env).expanduser().resolve()
    here = Path(__file__).resolve().parent
    linked = here / "bundle"
    if linked.exists():
        return linked.resolve()
    # Workspace fallback (dev container layout)
    candidate = Path(
        "/workspace/opensquilla-main/opensquilla-main/src/opensquilla/"
        "squilla_router/models/v4.2_phase3_inference"
    )
    return candidate


def _validate_bundle(bundle_dir: Path) -> None:
    required = ("runtime_src", "router.runtime.yaml", "lgbm_main.bin", "bge_onnx")
    missing = [name for name in required if not (bundle_dir / name).exists()]
    if missing:
        raise FileNotFoundError(f"missing V4 bundle files: {missing} under {bundle_dir}")
    # Reject Git LFS pointer files for the primary model
    main = bundle_dir / "lgbm_main.bin"
    head = main.read_bytes()[:40]
    if head.startswith(b"version https://git-lfs"):
        raise RuntimeError(
            f"bundle still contains Git LFS pointers at {main}; "
            "pull real weights first"
        )


def classify(req: dict) -> dict:
    import yaml

    bundle_dir = Path(req["bundle_dir"]) if req.get("bundle_dir") else default_bundle_dir()
    _validate_bundle(bundle_dir)

    runtime_src = str(bundle_dir / "runtime_src")
    if runtime_src not in sys.path:
        sys.path.insert(0, runtime_src)

    from src.router.inference.core import InferenceCore
    from src.router.inference.types import InferenceRequest

    config = yaml.safe_load((bundle_dir / "router.runtime.yaml").read_text(encoding="utf-8")) or {}
    use_aux = req.get("use_aux_head")
    if use_aux is None:
        use_aux = bool(config.get("v4", {}).get("aux_head_inference", False))

    core = InferenceCore.from_model_dir(str(bundle_dir), config, use_aux_head=bool(use_aux))

    message = str(req.get("message") or "")
    history_user_texts = [str(t) for t in (req.get("history_user_texts") or []) if t]
    prev_assistant_text = str(req.get("prev_assistant_text") or "") or None
    routing_history = req.get("routing_history") or []

    decisions = []
    for entry in routing_history:
        route_class = entry.get("final_route_class") or entry.get("route_class")
        if not route_class:
            continue
        decisions.append(
            SimpleNamespace(
                route_class=str(route_class),
                difficulty=float(entry.get("difficulty_score", entry.get("difficulty", 0.0)) or 0.0),
                margin=float(entry.get("margin", 0.0) or 0.0),
            )
        )

    context_tokens_est = max(
        0,
        (
            len(message)
            + sum(len(t) for t in history_user_texts)
            + len(prev_assistant_text or "")
        )
        // 4,
    )

    request = InferenceRequest(
        current_user_text=message,
        history_user_texts=history_user_texts,
        prev_assistant_text=prev_assistant_text,
        prev_assistant_usage=req.get("prev_assistant_usage"),
        prev_route_decisions=decisions,
        flags_text_override=req.get("flags_text_override"),
        context_metadata={
            "turn_index": int(req.get("turn_index") or len(routing_history)),
            "history_user_turn_count": len(history_user_texts),
            "context_tokens_est": context_tokens_est,
            "has_code_block": "```" in message,
            "has_prev_assistant": bool(prev_assistant_text),
        },
    )
    result = core.predict(request)
    decision = result.decision
    route_class = str(getattr(decision, "route_class", "R1"))
    # R0→c0 … R3→c3
    tier = "c" + route_class[-1] if route_class.startswith("R") else "c1"
    probs = getattr(result, "probabilities", {}) or {}
    confidence = float(probs.get(route_class, 0.0)) if probs else 0.0
    if confidence <= 0.0 and probs:
        confidence = float(max(probs.values()))
    difficulty = float(getattr(decision, "difficulty_score", 0.0) or 0.0)

    return {
        "ok": True,
        "tier": tier,
        "route_class": route_class,
        "confidence": confidence,
        "difficulty": difficulty,
        "source": "v4_phase3",
        "thinking_mode": str(getattr(decision, "thinking_mode", "T1")),
        "prompt_policy": str(getattr(decision, "prompt_policy", "P1")),
        "probabilities": {str(k): float(v) for k, v in probs.items()},
        "reasons": ["squilla_v4", f"route={route_class}"],
        "bundle_dir": str(bundle_dir),
    }


def main() -> int:
    try:
        raw = sys.stdin.read()
        req = json.loads(raw) if raw.strip() else {}
        out = classify(req)
    except Exception as exc:
        out = {
            "ok": False,
            "error": str(exc),
            "error_type": type(exc).__name__,
            "tier": "c1",
            "route_class": "R1",
            "confidence": 0.0,
            "difficulty": 0.0,
            "source": "v4_unavailable",
            "reasons": ["squilla_v4_error", type(exc).__name__],
        }
    sys.stdout.write(json.dumps(out, ensure_ascii=False))
    sys.stdout.write("\n")
    return 0 if out.get("ok") else 2


if __name__ == "__main__":
    raise SystemExit(main())
