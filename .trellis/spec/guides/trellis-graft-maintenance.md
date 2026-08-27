# Trellis Graft Maintenance

这个仓库的 Trellis 不是原版。workflow.md 与 5 个 agent 定义经过改造，
统一分发自 <https://github.com/Episkey-G/trellis-graft>。

改 `.trellis/`、`.claude/agents/`、`.claude/commands/trellis/` 之前先读这一页——
下面每条都是踩过之后验证出来的，不是推测。

---

## 改动必须落在 graft 仓库，不是这里

本仓库里那 8 个自定义文件是 **install.sh 的产物**。直接在这里改，下次跑
`install.sh` 就被覆盖，而且其他 13 个仓库拿不到。

正确顺序：改 graft 仓库 → 打 tag → 各仓库 `install.sh --target .`。

例外：临时验证可以先在本地改，验证完必须回灌 graft 仓库。

---

## Trellis 官方定义的安全编辑边界

来源：<https://docs.trytrellis.app/beta/advanced/custom-workflow.md>

解析器只认这五类结构，动它们就要同步改所有 runtime consumer：

| 结构 | 消费方 | 要求 |
| --- | --- | --- |
| `## Phase Index` | SessionStart | 精确标题；内容止于 `## Phase 1: Plan` |
| `## Phase 1: Plan` | SessionStart 边界 | 保持精确标题 |
| `#### X.Y` | `get_context.py --mode phase --step` | 数字 step id |
| `[workflow-state:STATUS]` 配对 | 每轮 hook | 开闭 STATUS 必须一致 |
| 平台标记配对 | phase 渲染器 | 开闭平台列表必须一致 |

**散文、路由指令、新增 phase、自定义状态都可以随便改。**

改完必须跑（这是官方推荐的唯一权威检查）：

```bash
for s in 1.0 1.1 1.2 1.3 1.4 1.5 2.1 2.2 2.3 3.2 3.3 3.4 3.5; do
  python3 ./.trellis/scripts/get_context.py --mode phase --step $s --platform claude-code
done
```

任何一个 step 返回空，就是标记配对断了。

---

## `--platform claude` 匹配不上任何东西

`_platform_matches` 做模糊匹配：转小写、去掉 `-` `_` 空格。
Claude Code 的块名是 `Claude Code` → `claudecode`，而 `dist/types/ai-tools.js` 里
它的 `cliFlag` 是 `"claude"` → `claude`。两者不等，**平台限定的正文被静默丢弃**：

```
--platform claude       -> 4 行（正文没了）
--platform claude-code  -> 18 行（正常）
```

截至 Trellis 0.6.14 上游未修。本仓库的 `/trellis:continue` 带的是修正值，
由 graft 分发。**其他平台不受影响**——cursor / codex / gemini / opencode 的
cliFlag 与块名模糊匹配后相等，实测全部正常。

---

## channel agent 的只读是软约束

`.trellis/agents/{check,implement}.md` 与 `.claude/agents/trellis-*.md` 是
**完全独立的两套**。改一套不影响另一套（`multi-agent-channel.md:65` 明确说明）。

关键限制：channel agent 是 platform-agnostic role card，frontmatter 只有
`name` / `description` / `provider` / `labels`，**没有 `tools` 字段**。
所以 check agent 的"只读"在 channel 里无法用工具收窄强制，只能靠指令措辞。

启用 `trellis channel` 之前，必须重新验证 worker 是否真的遵守。
（Codex 那边有 `sandbox_mode = "read-only"` 可以硬强制，Claude Code 靠收窄 `tools`，
channel 两者都没有。）

---

## `trellis update` 的 hash 分类

`dist/commands/update.js:662-679`。文件内容与新模板不同时：

- `storedHash` 存在 **且** 等于当前 hash → 自动覆盖（判定用户没改过）
- 其他所有情况（hash 不匹配 **或** 根本没有 storedHash）→ `Modified by you`，需确认

**hash 缺失和 hash 不匹配走同一分支。** 所以 `trellis init --workflow-source` 里的
`removeHash` 和"脚本改动导致 hash 对不上"效果一致——自定义文件不会被静默还原。

本仓库应有 8 项 modified：`config.yaml`、`workflow.md`、3 个 `.claude/agents/`、
`continue.md`、2 个 `.trellis/agents/`。多出来的项说明有人在本地改了不该改的东西。

---

## 升级顺序不能反

```bash
trellis update -s      # 1. 先让 scripts/hooks（解析器）到新版
trellis workflow -f    # 2. 再让新语法的 workflow.md 落地
install.sh             # 3. 最后是 agent 定义和文档
```

反过来会出现新语法配旧解析器，症状是断点或 phase 正文静默变空——不报错，
只是内容没了。`install.sh` 内建了这个顺序。

`-s` = skip all modified，跳过的正是我们那 8 个自定义文件，同时非交互。

---

## `trellis init` 不加 `-y` 会崩

不加 `-y` 会弹交互提示（statusLine 等），任何没有 TTY 的地方直接
`ERR_USE_AFTER_CLOSE`。写进任何脚本都必须带 `-y`。

---

## agent 与 skill 定义在会话启动时缓存

改完 `.claude/agents/*.md` 或 `.claude/skills/*/SKILL.md`，**当前会话不生效**。
必须开新会话才能验证。

这条踩过一次：上一轮改完只读 `trellis-check` 后直接跑端到端验证，跑的是缓存的旧定义，
两个 reviewer 都还有写权限，并发写了同一棵工作树。
**任何涉及 agent/skill 定义的验证，一律在新会话里做。**

---

## 被替换掉的旧技能

`trellis-brainstorm` 和 `trellis-check`（技能形态）仍在 `.claude/skills/` 里且可被
自动触发，但已被这套工作流替换。它们是 template-managed，删了会被
`trellis update` 加回来，所以处理方式是在 `AGENTS.md` 里声明"不要自动加载"，不改文件。

`trellis-check` 尤其要小心：**同名的技能会自我修复，agent 是只读的**。
验证一律用 Agent 形态。`workflow.md` 的 `[workflow-state:in_progress]` 里有一行消歧，
别删。
