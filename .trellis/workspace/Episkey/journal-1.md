# Journal - Episkey (Part 1)

> AI development session journal
> Started: 2026-08-07

---



## Session 1: Trellis 工作流嫁接补齐到标准形式，建立 trellis-graft 分发仓库

**Date**: 2026-08-07
**Task**: Trellis 工作流嫁接补齐到标准形式，建立 trellis-graft 分发仓库
**Branch**: `main`

### Summary

补齐 graft 的 5 处不一致（其中 trellis-check 技能/agent 同名消歧最关键——同名技能会自我修复，正好绕过双轴只读评审的设计）；建公开仓库 Episkey-G/trellis-graft 做统一分发，workflow.md 走官方 --workflow-source 通道，其余 8 项走 install.sh（接入=升级同一条命令），上游发新版跑 upgrade.sh 三方合并（BASE 存在 upstream/）；首次把 .trellis/ 纳入版本控制（PR #34）。双轴评审 14 条 finding 全部采纳；只读约束首次得到真实验证（零文件被写）。发现并修掉上游未修的 bug：Claude Code 的 cliFlag 是 claude 而解析器只认 claude-code，导致平台限定正文被静默丢弃。

### Git Commits

| Hash | Message |
|------|---------|
| `06643b3` | (see git log) |

### Status

[OK] **Completed**
