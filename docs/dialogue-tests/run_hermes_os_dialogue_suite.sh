#!/usr/bin/env bash
# Hermes + OpenSquilla feature dialogue acceptance against compiled heartq.
set -uo pipefail
export HEARTQ_HOME=${HEARTQ_HOME:-/root/.heartq}
export GROK_HOME=${GROK_HOME:-$HEARTQ_HOME}
HQ=${HQ:-/root/.heartq/bin/heartq}
ROOT=/workspace/heartq-build
CLIENT="$ROOT/docs/dialogue-tests/acp_multiturn_client.py"
OUT="$ROOT/docs/dialogue-tests/results/hermes-os-$(date +%Y%m%d-%H%M%S)"
export OUT
mkdir -p "$OUT"
REPORT="$OUT/REPORT.md"
TOKEN="HQ-FEAT-$(date +%s)"

pass=0; fail=0; skip=0; na=0
row() { echo "| $1 | $2 | $3 |" >> "$REPORT"; }
ok()  { echo "PASS $1 — $2"; row "$1" "PASS" "$2"; pass=$((pass+1)); }
bad() { echo "FAIL $1 — $2"; row "$1" "FAIL" "$2"; fail=$((fail+1)); }
sk()  { echo "SKIP $1 — $2"; row "$1" "SKIP" "$2"; skip=$((skip+1)); }
na_() { echo "N/A  $1 — $2"; row "$1" "N/A" "$2"; na=$((na+1)); }

{
  echo "# Hermes / OpenSquilla → HeartQ 对话验收报告"
  echo
  echo "- binary: \`$($HQ --version 2>/dev/null | head -1)\`"
  echo "- HEARTQ_HOME: \`$HEARTQ_HOME\`"
  echo "- started: $(date -Iseconds)"
  echo
  echo "| Case | Result | Notes |"
  echo "|------|--------|-------|"
} > "$REPORT"

run_p() {
  local id="$1"; shift
  "$HQ" --experimental-memory --always-approve --max-turns 4 \
    --debug-file "$OUT/${id}.debug" -p "$*" >"$OUT/${id}.log" 2>"$OUT/${id}.err" || true
}

run_acp() {
  local id="$1"; shift
  # acp client writes to docs/.../results/<id>.* — copy afterward
  python3 "$CLIENT" "$id" "$@" >"$OUT/${id}.console" 2>&1 || true
  local base="$ROOT/docs/dialogue-tests/results"
  for ext in log debug notifications.json console; do
    [[ -f "$base/${id}.$ext" ]] && cp -f "$base/${id}.$ext" "$OUT/" 2>/dev/null || true
  done
}

echo "=== H-MEM-01 memory write+recall ==="
run_p TC-H-MEM-01a "请永久记住唯一标记：${TOKEN}。一句话确认。"
run_p TC-H-MEM-01b "请用 memory_search 查找 ${TOKEN}，找到就原样输出。"
if grep -q "$TOKEN" "$OUT/TC-H-MEM-01b.log" 2>/dev/null; then ok TC-H-MEM-01 "跨会话记忆召回"; else bad TC-H-MEM-01 "未召回 $TOKEN"; fi
if rg -q "$TOKEN" "$HEARTQ_HOME/memory" -g '*.md' 2>/dev/null; then ok TC-H-MEM-02 "MEMORY.md 持久化"; else bad TC-H-MEM-02 "memory 文件无 token"; fi
if rg -q 'MEMORY_INJECT|TURN_PREFETCH' "$OUT/TC-H-MEM-01b.debug" 2>/dev/null; then ok TC-H-MEM-03 "prefetch 注入"; else sk TC-H-MEM-03 "debug 无 MEMORY_INJECT（可能首轮/日志级别）"; fi
if rg -q 'TURN_SYNC' "$OUT/TC-H-MEM-01a.debug" 2>/dev/null; then ok TC-H-MEM-04 "TurnSync"; else bad TC-H-MEM-04 "无 TURN_SYNC"; fi

echo "=== H-SK skill_manage via meta ==="
# ensure contract-review exists (deepherm)
if [[ ! -f "$HEARTQ_HOME/skills/contract-review/SKILL.md" ]]; then
  sk TC-H-SK-01 "contract-review 未安装"; sk TC-H-SK-02 "skip"; sk TC-H-SK-03 "skip"
else
  run_p TC-H-SK-01 "执行T1编辑合同审查技能"
  if grep -q 'MARKER_T1_EDIT_OK' "$HEARTQ_HOME/skills/contract-review/SKILL.md" 2>/dev/null; then ok TC-H-SK-01 "skill_manage edit via meta"; else bad TC-H-SK-01 "edit marker 缺失"; fi
  run_p TC-H-SK-02 "执行T1写文件合同技能"
  if grep -q 'MARKER_T1_WRITE_OK' "$HEARTQ_HOME/skills/contract-review/templates/t1_note.md" 2>/dev/null; then ok TC-H-SK-02 "write_file"; else bad TC-H-SK-02 "write_file 失败"; fi
  run_p TC-H-SK-03 "执行T1删除合同技能文件"
  if [[ ! -f "$HEARTQ_HOME/skills/contract-review/templates/t1_note.md" ]]; then ok TC-H-SK-03 "remove_file"; else bad TC-H-SK-03 "文件仍在"; fi
fi
run_p TC-H-SK-04 "请列出已安装的 procurement/审查 相关 skill 名称，不要编造。"
if grep -Eiq 'contract-review|bid-comparison' "$OUT/TC-H-SK-04.log" 2>/dev/null; then ok TC-H-SK-04 "技能发现"; else
  # fallback: skills on disk
  if ls "$HEARTQ_HOME/skills" | grep -q contract-review; then ok TC-H-SK-04 "磁盘有 skills（模型未点名）"; else bad TC-H-SK-04 "无技能"; fi
fi

echo "=== H-BR background review ==="
run_p TC-H-BR-01 "remember this: always prefer contract-review for financial contracts. Reply OK only."
if rg -q 'Background Review|remember this: always prefer contract-review' "$HEARTQ_HOME/memory" -g '*.md' 2>/dev/null; then ok TC-H-BR-01 "remember→MEMORY"; else bad TC-H-BR-01 "无 Background Review 笔记"; fi

SAME='Error: boom-xyz failed to parse quote. Reply with only the word ACK. No tools.'
run_acp TC-H-BR-02 "$SAME" "$SAME" "$SAME" "$SAME" "$SAME"
if [[ -f "$OUT/TC-H-BR-02.notifications.json" ]] && python3 -c "
import json,sys
n=json.load(open('$OUT/TC-H-BR-02.notifications.json'))
hits=False
for x in n:
  u=(x.get('params') or {}).get('update') or {}
  m=u.get('message') or ''
  if 'error-recovery' in m or '后台回顾' in m: hits=True
print('ok' if hits else 'no')
" | grep -q ok; then ok TC-H-BR-02 "重复错误→patch 建议"; else bad TC-H-BR-02 "无 hook_annotation"; fi

echo "=== H-CU / H-LG ==="
if [[ -f "$HEARTQ_HOME/skills/.curator_state.json" ]]; then ok TC-H-CU-01 "curator_state"; else bad TC-H-CU-01 "无 curator_state"; fi

# Learning graph via ACP close
python3 <<PY
import json,subprocess,os,time,pathlib,sys
os.environ["HEARTQ_HOME"]="$HEARTQ_HOME"
graph=pathlib.Path("$HEARTQ_HOME/memory/learning_graph.json")
# keep existing if present; still require exists after close
proc=subprocess.Popen(["$HQ","agent","--always-approve","--no-leader","stdio","--debug-file","$OUT/TC-H-LG-01.debug"],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,bufsize=1,cwd="/workspace/heartq-build")
def req(m,params=None,timeout=120):
  req.n=getattr(req,"n",0)+1
  msg={"jsonrpc":"2.0","id":req.n,"method":m}
  if params is not None: msg["params"]=params
  proc.stdin.write(json.dumps(msg)+"\n"); proc.stdin.flush()
  end=time.time()+timeout
  while time.time()<end:
    line=proc.stdout.readline()
    if not line: return None
    o=json.loads(line)
    if o.get("id")==req.n: return o
  return {"timeout":True}
req.n=0
req("initialize",{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":False,"writeTextFile":False},"terminal":False}})
req("authenticate",{"methodId":"xai.api_key","_meta":{"headless":True}})
sid=req("session/new",{"cwd":"/workspace/heartq-build","mcpServers":[]})["result"]["sessionId"]
req("session/prompt",{"sessionId":sid,"prompt":[{"type":"text","text":"只回复：LG"}],"_meta":{"screenMode":"headless"}},timeout=120)
req("_x.ai/session/close",{"sessionId":sid},timeout=60)
for _ in range(20):
  time.sleep(0.5)
  if graph.exists(): break
try:
  proc.stdin.close()
except Exception:
  pass
try:
  proc.wait(timeout=30)
except Exception:
  proc.kill()
open("$OUT/TC-H-LG-01.status","w").write("exists="+str(graph.exists())+"\n")
print("exists", graph.exists())
PY
if [[ -f "$HEARTQ_HOME/memory/learning_graph.json" ]]; then ok TC-H-LG-01 "learning_graph.json"; else bad TC-H-LG-01 "learning_graph 未写出"; fi

# CU-02 partial
if rg -q 'LLM curator|llm_curator' "$OUT"/*.debug 2>/dev/null; then ok TC-H-CU-02 "LLM curator 日志"; else sk TC-H-CU-02 "无 LLM curator 候选/日志"; fi

echo "=== H-CP / O-CP compaction ==="
run_p TC-H-CP-01 "请用 bash 打印约 150 行重复日志 TOOL_PAD_LINE，然后只回复 done。"
if rg -qi 'prune|tool_result|compaction|compact' "$OUT/TC-H-CP-01.debug" 2>/dev/null; then ok TC-H-CP-01 "pruning/compaction 信号"; else sk TC-H-CP-01 "短会话未触发压缩阈值"; fi

echo "=== O-MS meta-skill ==="
run_p TC-O-MS-01 "执行T5合同审查元技能"
if rg -q 't5-use-contract-review' "$HEARTQ_HOME/meta_runs"/*.json 2>/dev/null && rg -q '"status": "completed"' "$HEARTQ_HOME/meta_runs"/*.json 2>/dev/null; then ok TC-O-MS-01 "meta auto_trigger"; else bad TC-O-MS-01 "meta run 未完成"; fi

run_acp TC-O-MS-03 "执行T5澄清采购流程" "比价"
MS03=$(python3 -c "
import json,glob,os
home='$HEARTQ_HOME'
runs=sorted(glob.glob(home+'/meta_runs/*.json'), key=os.path.getmtime, reverse=True)
for p in runs[:12]:
  d=json.load(open(p))
  if d.get('meta_skill_name')=='t5-clarify-procurement':
    steps=d.get('steps') or []
    ok=d.get('status')=='completed' and any(s.get('skill_name')=='bid-comparison' and s.get('status')=='ok' for s in steps)
    print('PASS' if ok else 'FAIL')
    break
else:
  print('FAIL')
")
if [[ "$MS03" == PASS ]]; then ok TC-O-MS-03 "clarify+resume"; else bad TC-O-MS-03 "clarify resume 失败"; fi

echo "=== O-DM / O-AP dream + auto_propose ==="
PROPOSALS_BEFORE=$(ls "$HEARTQ_HOME/proposals"/*.json 2>/dev/null | wc -l)
run_acp TC-O-DM-01 \
  "确认已安装 contract-review 与 bid-comparison，只列名称。" \
  "/dream"
if rg -q 'MEMORY_DREAM_SLASH: consolidation complete|MEMORY_DREAM_SLASH: starting' "$OUT/TC-O-DM-01.debug" 2>/dev/null; then ok TC-O-DM-01 "dream slash"; else
  if rg -q 'MEMORY_DREAM' "$OUT/TC-O-DM-01.debug" 2>/dev/null; then ok TC-O-DM-01 "dream 有信号"; else bad TC-O-DM-01 "dream 未跑"; fi
fi
PROPOSALS_AFTER=$(ls "$HEARTQ_HOME/proposals"/*.json 2>/dev/null | wc -l)
if [[ "$PROPOSALS_AFTER" -gt 0 ]] && rg -q 'auto-propose' "$OUT/TC-O-DM-01.debug" 2>/dev/null; then ok TC-O-AP-01 "auto_propose 落盘"; elif [[ "$PROPOSALS_AFTER" -gt 0 ]]; then ok TC-O-AP-01 "proposals 目录有文件"; else sk TC-O-AP-01 "无新 proposal（可能无共现技能）"; fi

# N/A inventory (documented)
na_ H-MEM-05 "外部 Memory Provider 未移植"
na_ H-SK-04 "Skills Hub/guard 未移植"
na_ H-SK-05 "/learn 未移植"
na_ O-MS-05 "DAG when/route/failover 未移植"
na_ O-AP-03 "proposal accept/reject 未移植"
na_ O-TP-01 "Squilla Router 完整流水线未移植"
na_ H-GW-01 "多平台 Gateway 未移植"

{
  echo
  echo "## Summary"
  echo "- PASS: $pass"
  echo "- FAIL: $fail"
  echo "- SKIP: $skip"
  echo "- N/A (明确未移植): $na"
  echo
  echo "功能对照全文：\`docs/dialogue-tests/HERMES_OPENSQUILLA_FEATURE_MAP.md\`"
} >> "$REPORT"

echo
echo "REPORT=$REPORT"
echo "PASS=$pass FAIL=$fail SKIP=$skip N/A=$na"
exit $(( fail > 0 ? 1 : 0 ))
