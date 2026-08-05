# HeartQ T1–T9 对话式验收测试案例

> 依据计划 `heartq自启落地_1ce062fc`。Skills 来源：`/workspace/deepherm-master-experts-skills.zip`（已安装到 `~/.heartq/skills/`）。
> 执行方式：headless `heartq --experimental-memory --always-approve -p '...'`，副作用用文件/日志断言。

## 环境前置

```bash
export HEARTQ_HOME=/root/.heartq
# 二进制需含 T5–T9 符号
/root/.heartq/bin/heartq --version
# config 已启用 memory/background_review/curator/meta_skill_auto_trigger/auto_propose/compaction.pruning
# deepherm 12 个 skills 已安装；配套 meta_skills 已种子
```

| ID | 功能点 | 对话触发语 | 期望可观测结果 |
|----|--------|------------|----------------|
| TC-T1-01 | skill_manage edit | `执行T1编辑合同审查技能` | meta run completed；`contract-review/SKILL.md` 含 `MARKER_T1_EDIT_OK` |
| TC-T1-02 | skill_manage write_file | `执行T1写文件合同技能` | `contract-review/templates/t1_note.md` 含 `MARKER_T1_WRITE_OK` |
| TC-T1-03 | skill_manage remove_file | `执行T1删除合同技能文件` | `templates/t1_note.md` 被删除 |
| TC-T2-01 | background_review → memory note | `remember this: always use contract-review for 财务合同审查` | `MEMORY.md` 出现 Background Review / remember this |
| TC-T2-02 | background_review → skill patch 建议 | 多轮含 `Error: boom`×3 + interval | debug 或通知含「后台回顾建议」/`error-recovery`（当前实现为 HookAnnotation） |
| TC-T3-01 | curator idle 状态持久化 | 任意短对话后查状态 | `~/.heartq/skills/.curator_state.json` 存在且 `run_count>=1` |
| TC-T4-01 | LLM curator session end | 结束一轮会话 | debug 含 `LLM curator` 相关日志（dry_run 可能跳过 mutate） |
| TC-T5-01 | meta auto_trigger + deepherm 引用 | `执行T5合同审查元技能` | meta_runs 中 `t5-use-contract-review` status=completed |
| TC-T5-02 | clarify 暂停 | `执行T5澄清采购流程` | run awaiting/paused；通知含澄清问题 |
| TC-T5-03 | clarify 恢复 | 下一句 `比价`（同 session `-c`） | run 继续完成；引用 `bid-comparison` |
| TC-T6-01 | auto_propose /proposals | 多轮技能共现后 dream/end | `~/.heartq/proposals/` 有提案 **或** `/proposals` 可列（需 dream 条件） |
| TC-T7-01 | learning graph | session 正常结束 | `~/.heartq/memory/learning_graph.json` 生成 |
| TC-T8-01 | ToolResultPruner + compact | 产生超长工具输出后 `/compact` 或自动压缩 | debug 含 prune/compaction；上下文变短 |
| TC-T9-01 | 记忆跨会话召回 | 写唯一 token 再新会话搜 | memory_search 召回 |
| TC-T9-02 | deepherm skill 可被发现 | `列出当前可用的 procurement 相关 skills` | 回复含 contract-review / bid-comparison 等 |


## 执行记录（2026-07-30）

Skills 安装：12 个 deepherm leaf skills → `~/.heartq/skills/`。

套件：`docs/dialogue-tests/run_dialogue_suite.sh`  
报告：`docs/dialogue-tests/results/report-20260730-041905.md`

| Case | Result |
|------|--------|
| TC-T1-01/02/03 | PASS |
| TC-T2-01 | PASS |
| TC-T2-02 | SKIP（需同会话多轮） |
| TC-T3-01 | PASS |
| TC-T4-01 | PARTIAL |
| TC-T5-01/02 | PASS |
| TC-T5-03 | SKIP（需同会话 resume） |
| TC-T6-01 | SKIP |
| TC-T7-01 | FAIL（headless 跳过 session-end） |
| TC-T8-01 | PASS |
| TC-T9-01/02 | PASS |

**汇总：PASS 10 / FAIL 1 / SKIP|PARTIAL 4**

## 补测记录（2026-07-30 ACP 多轮）

| Case | Result |
|------|--------|
| TC-T2-02 | PASS（ACP 同会话重复错误） |
| TC-T5-03 | PASS（clarify → 比价 resume） |
| TC-T6-01 | PASS（`/dream` → proposals） |
| TC-T7-01 | PASS（`_x.ai/session/close` → learning_graph.json） |

详见 `results/report-retest-T2-T5-T6-T7.md`。
