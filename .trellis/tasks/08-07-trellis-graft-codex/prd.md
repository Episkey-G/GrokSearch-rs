# trellis-graft 支持 Codex 平台

> **状态：已排队，未开始规划。** 这份 PRD 只保存上一个任务
> （`archive/2026-08/08-07-trellis-workflow-standardize`）里已经查证过的事实，
> 避免下次重做调研。真正进 Phase 1 时按 `grill-with-docs` 重新收敛需求。

## 背景

`trellis-graft` 目前只支持 Claude Code：`install.sh` 的复制路径写死在 `.claude/` 下。
用户 2026-08-07 决定另开任务处理 Codex，理由是它需要真实 Codex 环境才能端到端验证，
而没验证就发布会跟现有的质量标准不一致。

## 已查证的事实 —— 不要重新调研

### 大头已经通用

`workflow.md` **完全平台无关**。它内建 18 个平台的路由块，Trellis 解析器
（`workflow_phase.py:_platform_matches`）对平台名做模糊匹配：转小写、去 `-` `_` 空格。
实测：

```
--platform codex     -> 18 行 正常
--platform cursor    -> 18 行 正常
--platform gemini    -> 18 行 正常
--platform opencode  -> 18 行 正常
--platform claude    ->  4 行 正文被丢（仅 Claude Code 有此问题）
```

`resolve_effective_platform` 会把 `--platform codex` 映射成 `codex-sub-agent`
（默认）或 `codex-inline`（`.trellis/config.yaml` 显式配置时）。

所以官方通道对 Codex 直接可用，零改动：

```bash
trellis init --codex --workflow trellis-mattpocock \
  --workflow-source gh:Episkey-G/trellis-graft
```

`install.sh` 现有 8 项产物里，通用的有 4 项：2 个 channel agent
（`.trellis/agents/`，platform-agnostic role card）+ 2 个 `docs/agents/`，
外加 `AGENTS.md` 段落。

### 真正要做的

1. **三个 sub-agent 定义移植成 TOML**。Codex 不用 `.md`——上游模板在
   `dist/templates/codex/agents/trellis-{check,implement,research}.toml`，
   字段是 `name` / `description` / `sandbox_mode` / `developer_instructions`，
   落在目标仓库的 `.codex/agents/`。
2. **`install.sh` 加 `--platform codex` 分支**，按平台选择复制 `.claude/agents/*.md`
   还是 `.codex/agents/*.toml`。
3. **`continue.md` 修复对 Codex 不需要**——Codex 的 `cliFlag` 是 `codex`，
   与块名模糊匹配后相等，不受那个上游 bug 影响。

### 一个比 Claude Code 更好的机会

Codex 的 agent 定义有 **`sandbox_mode` 字段**。Trellis 上游三个全填
`workspace-write`。把双轴评审那个改成 `read-only`，只读约束就是**沙箱硬强制**——
比 Claude Code 收窄 `tools` 更硬，比 channel role card 那种纯指令约束硬得多。
这是本次移植最有价值的部分，值得单独验证。

### 技能层

`npx skills` 支持 `codex` 作为安装目标（在其 75 个有效 agent 名单内）：

```bash
npx skills@latest add mattpocock/skills -g --copy -y -a codex -s ...
```

## 待定的问题

- Codex 的 `sandbox_mode` 合法值域需要查 Codex 官方文档确认（`read-only` /
  `workspace-write` / `danger-full-access` 是推断，未验证）
- 双轴 dispatch 在 Codex 上怎么传 `Axis:`——需要确认 Codex sub-agent 的
  dispatch prompt 机制
- 端到端验证需要真实 Codex 环境；没有的话这个任务不该发布

## 相关

- 分发仓库：<https://github.com/Episkey-G/trellis-graft>
- 维护契约：`.trellis/spec/guides/trellis-graft-maintenance.md`
- 上一个任务：`.trellis/tasks/archive/2026-08/08-07-trellis-workflow-standardize/`
