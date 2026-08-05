# heartq-model-router

Per-turn multi-model smart router for HeartQ. Selects a model tier (`c0`–`c3`)
for each user turn, then maps the tier to a HeartQ catalog model key.

## Strategies

| Strategy | Config value | Backend |
|----------|--------------|---------|
| Heuristic (v1) | `strategy = "heuristic"` | Pure Rust rules (default, no deps) |
| Squilla V4 (v2) | `strategy = "squilla_v4"` | OpenSquilla bundle via Python sidecar |

### SquillaRouter v2 setup

1. **Weights** — pull the V4 Phase 3 bundle (Git LFS), then symlink:

   ```bash
   # from an opensquilla checkout with real (non-pointer) weights:
   ln -sfn /path/to/opensquilla/.../v4.2_phase3_inference \
     crates/codegen/heartq-model-router/squilla-runtime/bundle
   ```

2. **Python env**:

   ```bash
   cd crates/codegen/heartq-model-router/squilla-runtime
   python3 -m venv .venv
   .venv/bin/pip install -r requirements.txt
   echo '{"message":"thanks"}' | .venv/bin/python classify.py
   ```

3. **HeartQ config**:

   ```toml
   [model_router]
   enabled = true
   strategy = "squilla_v4"
   rollout_phase = "observe"
   squilla_fallback_heuristic = true
   # optional overrides:
   # squilla_python = "/path/to/.venv/bin/python"
   # squilla_bundle_dir = "/path/to/v4.2_phase3_inference"
   ```

Env overrides: `HEARTQ_SQUILLA_PYTHON`, `HEARTQ_SQUILLA_SCRIPT`, `HEARTQ_SQUILLA_BUNDLE`,
`HEARTQ_MODEL_ROUTER=1`.

On sidecar failure, HeartQ falls back to the heuristic strategy when
`squilla_fallback_heuristic = true`.

> Note: v2 uses a **subprocess sidecar** (not in-process PyO3) so the HeartQ
> binary stays free of Python/`ort` link deps. Cold start loads ~40MB+ of models.

## Research summary

| Approach | Projects | Idea |
|----------|----------|------|
| Learned dual-model routing | [RouteLLM](https://github.com/lm-sys/RouteLLM) (ICLR 2025) | Train a classifier on preference data |
| Managed meta-routers | NotDiamond, Martian, OpenRouter Auto | Hosted endpoint + cost/quality dial |
| On-device tier routing | **OpenSquilla SquillaRouter** | Local LightGBM + ONNX → `c0`–`c3` |

## Architecture

```
User turn
  → RouterContext
  → strategy: heuristic | squilla_v4 (sidecar → InferenceCore)
  → postprocess (complaint / anti-downgrade / context floor)
  → tier c0–c3 → catalog model key
  → rollout_phase: observe | full
```

## Slash

`/router` — show config + last decision snapshot.
