#!/usr/bin/env bash
# Package a built release binary into an offline linux-arm64 tarball.
# Usage: ./scripts/pack-linux-arm64.sh [path/to/heartq-pager]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$ROOT/target/release/heartq-pager}"
[[ -x "$BIN" ]] || { echo "missing binary: $BIN" >&2; exit 1; }
VER="$("$BIN" --version 2>/dev/null | awk '{print $2; exit}')"
VER="${VER:-0.0.0}"
NAME="heartq-build-${VER}-linux-arm64"
DIST="$ROOT/dist"
STAGE="$DIST/$NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/share/examples"
cp -a "$BIN" "$STAGE/bin/heartq"
strip --strip-unneeded "$STAGE/bin/heartq" 2>/dev/null || true
chmod 755 "$STAGE/bin/heartq"
cp -a "$ROOT/LICENSE" "$STAGE/" 2>/dev/null || true
# Prefer checked-in intranet example configs when present.
if [[ -f "$ROOT/share/examples/config.toml.example" ]]; then
  cp -a "$ROOT/share/examples/config.toml.example" "$STAGE/share/examples/config.toml.example"
fi
if [[ -f "$ROOT/share/examples/config.toml" ]]; then
  cp -a "$ROOT/share/examples/config.toml" "$STAGE/share/examples/config.toml"
elif [[ -f "$STAGE/share/examples/config.toml.example" ]]; then
  cp -a "$STAGE/share/examples/config.toml.example" "$STAGE/share/examples/config.toml"
else
  cat > "$STAGE/share/examples/config.toml" << 'CFG'
[cli]
auto_update = false

[models]
default = "local-vllm"

[model.local-vllm]
model = "local-agent"
base_url = "http://127.0.0.1:8000/v1"
name = "Local vLLM"
api_backend = "chat_completions"
context_window = 32768
max_completion_tokens = 8192

[features]
telemetry = false
feedback = false
remote_fetch = false
CFG
fi
cat > "$STAGE/README.md" << 'MD'
# HeartQ Build linux-arm64

```bash
tar -xzf heartq-build-*-linux-arm64.tar.gz
cd heartq-build-*-linux-arm64
mkdir -p ~/.heartq
cp share/examples/config.toml.example ~/.heartq/config.toml
# edit base_url / model / context_window, then:
./bin/heartq
```

Intranet / local vLLM notes: see `share/examples/config.toml.example`.
MD
tar -C "$DIST" -czf "$DIST/${NAME}.tar.gz" "$NAME"
sha256sum "$DIST/${NAME}.tar.gz" | tee "$DIST/${NAME}.sha256"
ls -lh "$DIST/${NAME}.tar.gz"
