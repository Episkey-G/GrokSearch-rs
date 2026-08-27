# 执行计划

四个阶段，每个阶段末尾是一个回滚点。Stage B 前有一道人工闸门。

**贯穿约束**：工作树里已有约 3500 行未提交的业务改动（setup wizard / doctor CLI /
quality gate 等）。本任务的提交必须**按路径精确 add**，绝不 `git add -A`，
否则会把无关业务改动卷进 PR。

> **进度**：Stage A / B / C 已完成并验证；Stage D 的 D2 / D2a / D4 已完成，
> D1（新会话验证）与 D3（分支 + PR）待办。检查框反映真实状态。

---

## Stage A —— 本仓库修正到标准形式

全部落在本仓库，可独立验证，与 marketplace 无依赖。

- [x] **A1** `.trellis/workflow.md` `[workflow-state:in_progress]` 补 `trellis-check`
      技能/agent 消歧行（design.md「D1 / 1」的措辞）
- [x] **A2** `.trellis/workflow.md` inline 平台块 2.1：第 4 条改为经 `tdd` 逐切片
      red-green-refactor
- [x] **A3** `.trellis/workflow.md` inline 平台块 2.2：`trellis-check` 技能 → `code-review`
      技能，并注明它自行 spawn 两个评审 sub-agent
- [x] **A4** `.trellis/agents/check.md` 去 self-fix：frontmatter description、
      Core Responsibilities 第 4 条、Workflow 第 3 条的 fix-in-place 分支、
      Report Format 的 `Issues Found and Fixed` 段；补 Fowler smell 基线
- [x] **A5** `.trellis/agents/implement.md` 加 `tdd` 驱动语义
- [x] **A6** 装 `prototype` 技能（实测它本身不带 `disable-model-invocation`，无需剥离）：
      ```bash
      npx skills@latest add mattpocock/skills --copy -y -a claude-code -s prototype
      ```
- [x] **A7** `.claude/skills/implement/SKILL.md` frontmatter 加
      `disable-model-invocation: true`
- [x] **A8** `AGENTS.md` 的 `## Agent skills` 段：加「已被替换、不要自动加载」声明
      （`trellis-brainstorm`、`trellis-check` 技能形态），并用
      `<!-- MATTPOCOCK-GRAFT:START v=... -->` / `<!-- MATTPOCOCK-GRAFT:END -->` 包裹整段

### A 验证

```bash
# 13 个 step 全部非空
for s in 1.0 1.1 1.2 1.3 1.4 1.5 2.1 2.2 2.3 3.2 3.3 3.4 3.5; do
  n=$(python3 ./.trellis/scripts/get_context.py --mode phase --step $s --platform claude-code | wc -l)
  echo "step $s -> $n lines"
done

# 7 对 workflow-state 标签配对（只数真实标签，排除散文提及）
python3 - <<'PY'
import re
t = open('.trellis/workflow.md', encoding='utf-8').read()
op = re.findall(r'^\[workflow-state:([A-Za-z0-9_-]+)\]$', t, re.M)
cl = re.findall(r'^\[/workflow-state:([A-Za-z0-9_-]+)\]$', t, re.M)
print(op); print(cl); print('OK' if op == cl else 'MISMATCH')
PY

# modified 集合应为 8 项（原 6 + 新增 2 个 channel agent）
trellis update --dry-run
```

**通过标准**：13 个 step 非空；标签 `OK`；modified 列表恰为
`config.yaml` / `workflow.md` / 3 个 `.claude/agents/` / `continue.md` / 2 个 `.trellis/agents/`。

**实测结果**：13 个 step 全部非空；7 对标签配对（第 7 对 `my-status` 来自上游
「Adding a custom status」一节的文档示例，`workflow_phase.py:94` 会计数，属正常）；
平台标记 13/13；`in_progress` 断点 679B < 上游 872B；modified 恰为 8 项。

**回滚点 A**：`git checkout -- .trellis/workflow.md .trellis/agents/ AGENTS.md`
（`.claude/skills/` 下为未跟踪文件，需手工删）

---

## Stage B —— marketplace 仓库

- [x] **B0** 【人工闸门】确认创建公开仓库 `Episkey-G/trellis-graft`。
      创建公开仓库是对外操作，未确认不得执行 `gh repo create`
- [x] **B1** 在 scratchpad 里搭出 design.md 的目录结构
- [x] **B2** 把 Stage A 后的产物复制进去：`workflow.md` → `workflows/trellis-mattpocock/`；
      3 个 `.claude/agents/` → `agents/claude/`；2 个 `.trellis/agents/` → `agents/channel/`；
      2 个 `docs/agents/` → `docs/agents/`；AGENTS.md 段落 → `snippets/agents-md-section.md`
- [x] **B3** 写 `index.json`（design.md 给了完整内容）、`VERSION`（`1.0.0`）、
      `LICENSE`（AGPL-3.0-only 全文，直接取自 npm 包）、`NOTICE`（署名 mindfold-ai/Trellis）、
      `README.md`（四种场景各一节）
- [x] **B3a** 建 `upstream/` 基线——三方合并的 BASE。从当前已安装的 0.6.14 包里提取
      8 个上游原版模板，`upstream/VERSION` 写 `0.6.14`：
      ```bash
      P=$(dirname $(readlink -f $(which trellis)))/../dist/templates
      # trellis/workflow.md, trellis/agents/{check,implement}.md,
      # claude/agents/trellis-{implement,check,research}.md,
      # common/commands/continue.md
      ```
- [x] **B3b** 写 `upgrade.sh`：升级全局 CLI → `npm pack` 取新原版 → 逐文件
      `git merge-file --diff3`（BASE=`upstream/`，OURS=我方版本，THEIRS=新原版）→
      内联校验 → 刷新 BASE 并递增版本。有冲突则停在第 3 步，`--continue` 从第 4 步接上。
      **校验逻辑内联，不单独出 `validate.sh`**——它只被这一处调用，单独成文件是多余的间接层
- [x] **B4** `gh repo create Episkey-G/trellis-graft --public` 并推送，打 tag `v1.0.0`

### B 验证

```bash
trellis workflow --list -m gh:Episkey-G/trellis-graft

# 三方合并冒烟：把 BASE 临时退回 0.6.7（真实存在 39 行上游变更），跑一次真升级
./upgrade.sh 0.6.14
```

**通过标准**：`--list` 输出中出现 `trellis-mattpocock (marketplace)`；
冲突全部落在 step 正文内，`## Phase Index` 与 7 对 workflow-state 块零冲突。

**实测结果**：`--list` 正常列出；BASE=0.6.14（无变化）路径 7 个文件全部
"upstream unchanged"；BASE=0.6.7 路径产生 4 个冲突，全在 `#### 1.2` / `#### 2.1` /
`#### 2.2` 三个 step 正文内，与规划阶段手工三方合并的结果一致；`--continue` 的
守卫（冲突未解则拒绝）和恢复（从第 4 步接上）均验证通过。

**回滚点 B**：`gh repo delete Episkey-G/trellis-graft`（或改私有）。
本仓库未受影响。

---

## Stage C —— install.sh 与端到端验证

- [x] **C1** 按 design.md「D3 install.sh 契约」写脚本：
      `--target` / `--ref` / `--platform` / `--dry-run`；前置校验；
      `git clone --depth 1` 取源；8 项产物；AGENTS.md 按 marker 幂等替换；
      绝不自己写 workflow.md（改为调用官方命令）；结束打印手动收尾两件事
- [x] **C1a** 【实施中途新增】把 `.claude/commands/trellis/continue.md` 加入分发清单——
      上游 `cliFlag` bug 使平台限定正文被静默丢弃，规格原本只列了 7 项。
      同步更新 prd / design 的数量表述
- [x] **C2** scratchpad 里建一次性空仓库，跑完整 `install.sh --target <tmp>`
      （它自己调 `trellis init -y`）
- [x] **C3** 对该空仓库跑两遍 `install.sh --target <tmp>`
- [x] **C4** 用完删除临时仓库

### C 验证

```bash
diff <tmp>/.trellis/workflow.md .trellis/workflow.md
python3 -c "
import json; h=json.load(open('<tmp>/.trellis/.template-hashes.json'))['hashes']
print('workflow.md in hashes:', '.trellis/workflow.md' in h)"
grep -c 'MATTPOCOCK-GRAFT:START' <tmp>/AGENTS.md
```

**通过标准**：`diff` 无输出；`workflow.md in hashes: False`；marker 计数为 `1`。

**实测结果**：全部通过。另外 `step 2.2 --platform claude-code` 渲染 18 行（确认
`continue.md` 修复生效）；两遍之后 127 个文件指纹完全一致（差异仅在
`trellis update` 自建的 `.trellis/.backup-<时间戳>/`，非脚本产物）。

**首轮失败并已修复**：第一次跑直接崩在 `ERR_USE_AFTER_CLOSE`——`trellis init`
不加 `-y` 会弹交互提示，非 TTY 必挂。看代码看不出来，只有真跑才发现。

**证据留存的教训**：C4 删掉临时仓库后，AC7 / AC8 的验证结果无法被后续评审复核。
下次这类验证应保留指纹文件或日志，不要一删了之。

**绝对边界**：只在 scratchpad 的一次性仓库中验证，**不碰 13 个真实仓库中的任何一个**。

**回滚点 C**：删临时仓库即可，无外部副作用。

---

## Stage D —— 收尾

- [ ] **D1** 开新会话确认 `trellis-check` agent 工具集为 `Read, Bash, Glob, Grep`
      （agent 定义在会话启动时缓存，本会话看到的可能是旧的）
- [x] **D2** Phase 3.3 spec update：`.trellis/spec/guides/trellis-graft-maintenance.md`
      —— 解析契约、`--platform claude` 的坑、channel 只读是软约束、hash 分类、
      `trellis init` 需要 `-y`、会话缓存、被替换的旧技能
- [x] **D2a** 同时写进 spec：上游升级的正确顺序是 `trellis update`（先让 scripts/hooks
      到新版）→ 拉新 workflow.md → `install.sh`。顺序反了会出现新语法配旧解析器
- [x] **D2b** Phase 2.2 双轴评审已跑，10 + 4 条 finding 全部采纳并修复；
      **只读约束首次得到真实验证**（零文件被写，handoff 里挂了两轮的闸口关闭）
- [ ] **D3** 开分支，**按路径精确 add**，提 PR，不直推 main。
      **前置决定**：`.trellis/` 是否首次纳入版本控制（目前 0 个文件已跟踪；
      `.claude/` 已在 `.gitignore` 中）
- [x] **D4** 更新 memory 里的 `proj-trellis-mattpocock-graft`：记录 graft 仓库地址、
      0.6.14 已有 `init --workflow-source`（修正 handoff 里「0.7 才有」的错误结论）

---

## 明确不做

- 13 个仓库的实际 rollout —— 另开 task
- **Codex 平台支持** —— 另开 task（用户 2026-08-07 决定）。`.codex/agents/*.toml`
  三个定义 + `install.sh` 的 codex 分支；`sandbox_mode = "read-only"` 可硬强制只读
- 升级 0.7.0-beta
- 填充 `.trellis/spec/backend/*.md` 占位符
- 处理工作树里那约 3500 行未提交的业务改动
