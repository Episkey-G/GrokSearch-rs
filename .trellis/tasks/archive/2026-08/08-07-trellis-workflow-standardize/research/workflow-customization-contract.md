# Trellis 官方自定义 workflow 契约与分发能力

调查时间：2026-08-07。本地 CLI 与项目版本均为 0.6.14。
一手来源：CLI `--help` 实测输出、`dist/` 源码、官方文档 docs.trytrellis.app。

---

## 1. 官方定义的 workflow 解析契约

来源：<https://docs.trytrellis.app/beta/advanced/custom-workflow.md>

| 结构 | 消费方 | 要求 |
| --- | --- | --- |
| `## Phase Index` | SessionStart | 精确标题；内容止于 `## Phase 1: Plan` |
| `## Phase 1: Plan` | SessionStart 边界 | 保持精确标题 |
| `#### X.Y` | `get_context.py --mode phase --step` | 数字 step id |
| `[workflow-state:STATUS]` 配对 | 每轮 hook | 开闭 STATUS 必须一致 |
| 平台标记配对 | phase 渲染器 | 开闭平台列表必须一致 |

标准 workflow-state id：`no_task` / `planning` / `planning-inline` / `in_progress` /
`in_progress-inline` / `completed`。自定义 id 允许 `[A-Za-z0-9_-]+`，但只有当某个任务生命周期
路径写入对应的 `task.json.status` 时才会激活。

官方原文界定的安全编辑边界：可以自由改散文、加 phase、改路由指令、加自定义状态；
除非同时更新每一个 runtime consumer，否则必须保留解析语法。

验证命令（官方推荐，也是唯一权威检查）：

```bash
python3 ./.trellis/scripts/get_context.py --mode phase
python3 ./.trellis/scripts/get_context.py --mode phase --step 2.1
python3 ./.trellis/scripts/get_context.py --mode phase --step 2.1 --platform claude-code
```

补充：本地 `.claude/skills/trellis-meta/references/customize-local/change-workflow.md`
第 3 条要求「新增或重命名 skill/agent 时，同步平台目录下对应文件」。该文件本身滞后于实现
（它写的是 `Skill Routing` 表，0.6.14 里实际是 `Active Task Routing`）。

---

## 2. 0.6.14 已有的分发能力（实测）

`trellis init --help`：

```
--workflow <id>             Workflow template id for .trellis/workflow.md
                            (default: native; e.g., tdd, channel-driven-subagent-dispatch)
--workflow-source <source>  Custom marketplace source for the --workflow lookup
                            (e.g., gh:myorg/myrepo/marketplace)
```

`trellis workflow --help`：`-t/--template <id>`、`-m/--marketplace <source>`、`--list`、
`-f/--force`、`-n/--create-new`。**没有** `--save`。

关键实现细节 `dist/commands/init.js:1530`：使用非 native workflow 时，init 会
`removeHash(cwd, PATHS.WORKFLOW_GUIDE_FILE)` 丢弃 `.trellis/workflow.md` 的 hash 条目，
使 `trellis update` 将其判定为 modified 而非静默还原成 native 字节。注释明确引用
design.md "Durable-state contract"——即自定义 workflow 是官方支持的一等场景。

marketplace schema（<https://raw.githubusercontent.com/mindfold-ai/marketplace/main/index.json>）：

```json
{ "version": 1, "templates": [
  { "id": "<id>", "type": "workflow", "name": "...", "description": "...",
    "path": "workflows/<id>/workflow.md", "tags": ["workflow"] } ] }
```

source 前缀支持 `gh:` / `github:` / `gitlab:` / `bitbucket:` / 裸 HTTPS
（`dist/utils/template-fetcher.js:44`）。

**覆盖范围**：`--workflow` / `trellis workflow` 只写 `.trellis/workflow.md` 一个文件。
agent 定义、skills、commands、docs 均不在其中。

---

## 3. 0.7 才有的能力（本地 0.6.14 尚不可用）

来源：<https://docs.trytrellis.app/beta/advanced/dynamic-workflow-switching.md>
npm dist-tag：`beta = 0.7.0-beta.3`，`latest = 0.6.14`。

- `trellis workflow create <id>` —— 从 bundled native 生成 `.trellis/workflows/<id>.md` 脚手架
- `trellis workflow --save <id>` —— 把 marketplace 变体存进本地库；`trellis update` 不覆盖该库
- 四层优先级解析链：
  1. `task.json` 的 `workflow`
  2. `.trellis/.developer` 的 `workflow=`（gitignored，个人覆盖）
  3. `.trellis/config.yaml` 的 `default_workflow`（团队默认，入库）
  4. `.trellis/workflow.md`（项目兜底）
- `task.py create --workflow <id>` / `task.py workflow <id>` / `task.py workflow --clear`

已知限制（官方列出）：Oh My Pi 扩展仍只读全局 workflow.md；OpenCode 的 SessionStart 摘要
仍读全局；Snow 的 SessionStart 与每轮上下文仍读全局；保存的 marketplace workflow 是本地副本，
不 `--save --force` 就不更新。

---

## 4. 对本次 graft 的结论

本仓库的改动全部落在官方 "safe editing boundary" 内：只改了散文与路由指令，
五类解析结构原样保留。实测 7 对 `[workflow-state:*]` 标签配对完好、平台标记配对完好、
13 个 step 用 `--platform claude-code` 渲染全部非空。

唯一偏离 sanctioned 流程之处：change-workflow.md 第 3 条要求同步平台目录下的 agent 文件，
`.claude/agents/trellis-*.md` 三个已同步，`.trellis/agents/{check,implement}.md`
（channel runtime 的第二套定义）未同步，与 ulog 原版逐字节相同。
