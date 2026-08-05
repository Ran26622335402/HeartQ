# Hermes / OpenSquilla → HeartQ 功能点对照与对话验收

> 源码拆解：`/workspace/hermes-agent-main`、`/workspace/opensquilla-main/opensquilla-main`  
> 被测二进制：`/root/.heartq/bin/heartq`（0.2.109）  
> 方式：ACP `heartq agent stdio` 多轮对话 + 副作用断言

## 图例

| 标记 | 含义 |
|------|------|
| **HQ✅** | HeartQ 已接线，可用对话验收 |
| **HQ△** | 部分实现 / 需特定路径（meta/session-close/dream） |
| **HQ❌** | 未移植或产品形态不同，对话测 N/A |

---

## A. Hermes 功能点 → HeartQ 映射

### A1 Memory

| ID | Hermes 功能 | HQ | 对话用例 |
|----|-------------|----|----------|
| H-MEM-01 | 跨会话记忆写入/召回 | ✅ | `TC-H-MEM-01` 记住唯一 token → 新会话 `memory_search` |
| H-MEM-02 | MEMORY.md 持久化 | ✅ | 断言 `~/.heartq/memory/**/MEMORY.md` |
| H-MEM-03 | 回合 prefetch 注入 | ✅ | debug 含 `MEMORY_INJECT` / `TURN_PREFETCH` |
| H-MEM-04 | 回合 TurnSync | ✅ | debug 含 `TURN_SYNC: persisted` |
| H-MEM-05 | 外部 Memory Provider（Honcho/Mem0…） | ❌ | N/A |
| H-MEM-06 | memory write_approval | ❌ | N/A |

### A2 Skills

| ID | Hermes 功能 | HQ | 对话用例 |
|----|-------------|----|----------|
| H-SK-01 | skill_manage create/edit/patch/delete | △ | 经 meta `skill_manage` 步（非默认 agent 原生 tool）`TC-H-SK-01` |
| H-SK-02 | write_file / remove_file | △ | meta 触发 `TC-H-SK-02/03` |
| H-SK-03 | 技能发现与列表 | ✅ | 询问 procurement skills → 含 contract-review 等 |
| H-SK-04 | Skills Hub / guard | ❌ | N/A |
| H-SK-05 | `/learn` 蒸馏 | ❌ | N/A |

### A3 Compaction

| ID | Hermes 功能 | HQ | 对话用例 |
|----|-------------|----|----------|
| H-CP-01 | ToolResultPruner / pruning | ✅ | 超长工具输出后 debug 含 prune/compaction `TC-H-CP-01` |
| H-CP-02 | `/compact` 手动压缩 | △ | `/compact` 斜杠（若 agent 面可用）`TC-H-CP-02` |
| H-CP-03 | 可插拔 Context Engine | ❌ | N/A |

### A4 Background Review / Curator / Learning

| ID | Hermes 功能 | HQ | 对话用例 |
|----|-------------|----|----------|
| H-BR-01 | remember 短语 → 记忆笔记 | ✅ | `remember this: …` → MEMORY Background Review `TC-H-BR-01` |
| H-BR-02 | 重复错误 → skill patch 建议 | ✅ | 同会话相同 Error×≥3 → hook_annotation `TC-H-BR-02` |
| H-CU-01 | Curator idle 状态持久化 | ✅ | `.curator_state.json` 存在 `TC-H-CU-01` |
| H-CU-02 | LLM curator consolidation | △ | session-end 日志（可能 dry_run）`TC-H-CU-02` |
| H-LG-01 | Journey / Learning Graph | △ | session close → `learning_graph.json` `TC-H-LG-01` |

### A5 外围（网关/TUI/多 Provider）

Gateway 多平台、pairing、cron、desktop、ACP IDE 等 → **HQ❌** 不纳入本轮对话验收。

---

## B. OpenSquilla 功能点 → HeartQ 映射

### B1 Meta-skill

| ID | OS 功能 | HQ | 对话用例 |
|----|---------|----|----------|
| O-MS-01 | trigger 软激活 auto_trigger | ✅ | `hello meta` / `执行T5…` → meta_runs `TC-O-MS-01` |
| O-MS-02 | SkillManagerExecutor 执行步 | ✅ | create/edit 落盘 `TC-O-MS-02` |
| O-MS-03 | clarify 暂停 | ✅ | `执行T5澄清采购流程` → awaiting `TC-O-MS-03` |
| O-MS-04 | clarify resume | ✅ | 同会话下一句 `比价` → completed `TC-O-MS-04` |
| O-MS-05 | DAG 并行 / when / route / on_failure | ❌ | N/A（线性 runner） |
| O-MS-06 | SQLite meta_run 审计 + replay CLI | △ | JSON meta_runs 文件可查，无完整 CLI replay |

### B2 Dream / Auto-propose

| ID | OS 功能 | HQ | 对话用例 |
|----|---------|----|----------|
| O-DM-01 | Dream 整合（/dream 或 session-end） | ✅ | `/dream` → consolidation complete `TC-O-DM-01` |
| O-AP-01 | auto_propose 落盘 | ✅ | dream 后 `~/.heartq/proposals/*.json` `TC-O-AP-01` |
| O-AP-02 | `/proposals` 列表 | △ | pager 命令（stdio 可用则测）`TC-O-AP-02` |
| O-AP-03 | accept/reject 晋升 | ❌ | N/A |

### B3 Compaction / Turn Pipeline

| ID | OS 功能 | HQ | 对话用例 |
|----|---------|----|----------|
| O-CP-01 | 工具结果剪枝 + 压缩门控 | ✅ | 同 H-CP-01 |
| O-TP-01 | 完整 8 阶段 TurnRunner / Squilla Router | ❌ | N/A（HeartQ ACP SessionActor） |

---

## C. 对话测试用例（可执行）

### 环境

```bash
export HEARTQ_HOME=/root/.heartq
export GROK_HOME=/root/.heartq
HQ=/root/.heartq/bin/heartq
# 多轮：docs/dialogue-tests/acp_multiturn_client.py
```

| Case | 对应功能 | 步骤 | 期望 |
|------|----------|------|------|
| TC-H-MEM-01 | H-MEM-01/02/03 | 写 TOKEN → 新会话召回 | PASS 若召回成功 |
| TC-H-SK-01 | H-SK-01 | `执行T1编辑合同审查技能` | SKILL.md 含 MARKER |
| TC-H-SK-02 | H-SK-02 write | `执行T1写文件合同技能` | templates/t1_note.md |
| TC-H-SK-03 | H-SK-02 remove | `执行T1删除合同技能文件` | 文件删除 |
| TC-H-SK-04 | H-SK-03 | 列出 procurement skills | 含 contract-review |
| TC-H-BR-01 | H-BR-01 | remember this: … | MEMORY 有 Background Review |
| TC-H-BR-02 | H-BR-02 | 同会话相同 Error×5 | hook_annotation error-recovery |
| TC-H-CU-01 | H-CU-01 | 查 curator_state | 文件存在 |
| TC-H-LG-01 | H-LG-01 | 对话后 `_x.ai/session/close` | learning_graph.json |
| TC-H-CP-01 | H-CP-01 | 超长 bash 输出 | debug prune/compact 信号 |
| TC-O-MS-01 | O-MS-01 | `执行T5合同审查元技能` | meta_runs completed |
| TC-O-MS-03/04 | O-MS-03/04 | clarify → 比价 | run completed |
| TC-O-DM-01 | O-DM-01 | `/dream` | MEMORY_DREAM_SLASH complete |
| TC-O-AP-01 | O-AP-01 | dream 后查 proposals | 有 json |

### 明确不测（HQ❌）

- Hermes：多平台 Gateway、Memory Provider 插件、Skills Hub、`/learn`、clarify 网关卡片  
- OpenSquilla：DAG when/route/failover、SQLite replay CLI、proposal accept/reject、Squilla Router  

---

## D. 执行入口

```bash
bash /workspace/heartq-build/docs/dialogue-tests/run_hermes_os_dialogue_suite.sh
```
