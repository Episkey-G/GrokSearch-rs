# 技术设计

## 边界

两条分发通道，各管各的，不重叠：

```
                   Episkey-G/trellis-graft (公开, AGPL-3.0)
                   ├── index.json ──┐
                   │  workflows/    │ 官方通道
                   └── agents/ ─────┼─ 我方通道
                      docs/         │
                      install.sh ───┘
                             │
        ┌────────────────────┴────────────────────┐
        ▼                                          ▼
  trellis init --workflow <id>              install.sh --target <repo>
  --workflow-source gh:...                  拉齐 8 项
  或 trellis workflow -m ... -t ...          绝不碰 workflow.md
        │
        ▼
  .trellis/workflow.md  (init 会 removeHash，保证 update 判 modified)
```

**为什么 install.sh 不碰 workflow.md**：workflow.md 有官方通道（`trellis init --workflow-source`
与 `trellis workflow -m -t`），两条通道各管各的，避免同一个文件有两个写入者。

> **更正**：早先记录的理由是「只有官方命令维护得对 hash 语义」，这个说法不成立。
> 已核对 `dist/commands/update.js:662-679`：文件内容与新模板不同时，只有
> `storedHash 存在且等于当前 hash` 才自动覆盖；**hash 缺失与 hash 不匹配走同一分支**，
> 都进 "Modified by you"。所以 `init` 的 `removeHash` 与「脚本改动导致 hash 对不上」
> 结果一致，脚本写 workflow.md 不会破坏契约。保持分工是为了单一写入者，不是技术强制。

## D1：五处修正

### 1. `trellis-check` 消歧（`.trellis/workflow.md`）

落点 `[workflow-state:in_progress]`。上游原文有一行 Tools 消歧，graft v1 保留了变体，
v2 整行删除。补回一行，措辞覆盖三个同名/易混项：

```
Tools: `trellis-implement` / `trellis-check` / `trellis-research` 是 sub-agent 类型（Task/Agent 工具）。
同名的 `trellis-check` 技能是被本工作流替换掉的旧单体检查，会自我修复 —— 验证时一律用 Agent 形态。
```

体积预算：当前该块 439B，上游同块 872B，加一行约 +140B，仍在预算内。

### 2. channel runtime agent 对齐（`.trellis/agents/{check,implement}.md`）

**约束**：channel agent 是 platform-agnostic role card，frontmatter 只有
`name` / `description` / `provider` / `labels`，**没有 `tools` 字段**。只读无法用工具收窄，
只能靠指令。`multi-agent-channel.md:65` 明确说明两套 agent 完全独立，改 `.claude/agents/`
不影响 channel worker。

**对齐边界**——对齐语义，不对齐结构：

| 项 | 是否对齐 | 理由 |
| --- | --- | --- |
| check 去掉 self-fix，改为只报告 | 是 | 用户故事 7 的核心；两套 agent 一套只读一套自修复是真正的隐患 |
| check 携带 Fowler smell 基线 | 是 | 评审质量的实质内容，与调用方式无关 |
| check 拆成双轴 | **否** | 双轴靠两次 dispatch 传 `Axis:` 实现，channel 是 `--agent check` 单 spawn；且本仓库不使用 workflow 的 channel 分支。属于未要求的灵活性 |
| implement 驱动 `tdd` | 是 | 与 `.claude/agents/trellis-implement.md` 同一语义 |
| implement 加 `Skill` 工具 | 不适用 | 无 `tools` 字段；channel worker 的工具由 provider 决定 |

check.md 需删除的具体内容：frontmatter description 里的 `self-fixes issues`、
Core Responsibilities 第 4 条 `Self-fix`、Workflow 第 3 条的 `→ fix in-place` 分支、
Report Format 里的 `Issues Found and Fixed` 段。

### 3. inline 平台块对齐（`.trellis/workflow.md`）

`[workflow-state:in_progress-inline]` 已声明 `trellis-before-dev -> tdd -> code-review`，
但同文件的 step 正文仍是旧的。两处编辑：

- **2.1 inline 块**：第 4 条 `Implement the code per reviewed artifacts`
  → 改为经由 `tdd` 逐个行为切片 red-green-refactor
- **2.2 inline 块**：`Load the trellis-check skill` → `Load the code-review skill`，
  并说明它会自行 spawn 两个评审 sub-agent（inline 模式下唯一允许的 spawn）

1.2 inline 块（"在主会话直接做研究"）保持不变——inline 模式的定义就是把工作留在主会话，
与上游一致，不是遗漏。

### 4. 安装 `prototype` 技能

```bash
npx skills@latest add mattpocock/skills --copy -y -a claude-code -s prototype
```

装完剥掉 `disable-model-invocation`（与既有 11 个一致）。技能总数 11 → 12。

### 5. 旧技能抑制

分两种手法，避免给 `trellis update` 增加决策项：

| 技能 | template-managed | 手法 |
| --- | --- | --- |
| `trellis-brainstorm` | 是 | AGENTS.md 声明，不改文件 |
| `trellis-check`（技能形态） | 是 | AGENTS.md 声明 + workflow.md 消歧行（修正 1） |
| `implement`（mattpocock） | 否 | 直接加 `disable-model-invocation: true` |

`trellis-break-loop` 不处理——workflow.md 3.2 明确保留它作为 fallback，是有意设计。

## D2：marketplace 仓库结构

```
trellis-graft/
├── index.json                 # 官方 schema，只声明 workflow 条目
├── VERSION                    # graft 版本号，install.sh 读它写进 AGENTS.md
├── LICENSE                    # AGPL-3.0-only
├── NOTICE                     # 署名 mindfold-ai/Trellis
├── README.md                  # 两条通道的用法
├── upstream/                  # graft 所基于的上游原版 —— 三方合并的 BASE
│   ├── VERSION                #   0.6.14
│   ├── trellis/workflow.md
│   ├── trellis/agents/{check,implement}.md
│   ├── claude/agents/trellis-{implement,check,research}.md
│   └── common/commands/continue.md
├── workflows/
│   └── trellis-mattpocock/
│       └── workflow.md        # ← 官方通道读这个
├── agents/
│   ├── claude/                # → 目标仓库 .claude/agents/
│   │   ├── trellis-implement.md
│   │   ├── trellis-check.md
│   │   └── trellis-research.md
│   └── channel/               # → 目标仓库 .trellis/agents/
│       ├── check.md
│       └── implement.md
├── docs/agents/               # → 目标仓库 docs/agents/
│   ├── issue-tracker.md
│   └── domain.md
├── commands/claude/trellis/   # → 目标仓库 .claude/commands/trellis/
│   └── continue.md            #   修掉上游 --platform claude 的 bug
├── snippets/
│   └── agents-md-section.md   # → 注入目标仓库 AGENTS.md
├── upgrade.sh                 # 本仓库侧唯一命令：跟进上游（三方合并）
└── install.sh                 # 目标仓库侧唯一命令：接入 = 升级
```

`index.json`：

```json
{
  "version": 1,
  "templates": [
    {
      "id": "trellis-mattpocock",
      "type": "workflow",
      "name": "Trellis × mattpocock Engineering Skills",
      "description": "Trellis phase machine driving mattpocock/skills: grill-with-docs → to-spec → to-tickets in Phase 1; tdd via trellis-implement and a two-axis read-only trellis-check in Phase 2",
      "path": "workflows/trellis-mattpocock/workflow.md",
      "tags": ["workflow", "tdd", "grilling", "sub-agent", "two-axis-review"]
    }
  ]
}
```

`gh:Episkey-G/trellis-graft` 解析后 `subdir` 为空、`ref` 默认 `main`，
index.json 落在仓库根——已核对 `template-fetcher.js:203-210` 的解析规则，两段
`user/repo` 即为合法 source，subdir 可选。

## D3：install.sh 契约 —— 目标仓库侧的唯一命令

```
install.sh --target <repo-path> [--ref <git-ref>] [--platform claude] [--dry-run]
```

一条命令覆盖三种场景，脚本自己判断走哪条：

| 场景 | 判定条件 | 脚本做什么 |
| --- | --- | --- |
| 全新仓库 | `<target>/.trellis/` 不存在 | `trellis init --<platform> --workflow trellis-mattpocock --workflow-source gh:Episkey-G/trellis-graft` → 复制 8 项 |
| 首次接入（已有 stock Trellis） | `.trellis/` 存在，无 graft marker | `trellis update -s` → `trellis workflow -m ... -t ... -f` → 复制 8 项 |
| 后续升级（已有 graft） | AGENTS.md 里有 `MATTPOCOCK-GRAFT` marker | 同上；marker 里的版本号变成新版 |

后两种场景的动作完全一样，所以脚本不需要 `--upgrade` 之类的开关——**接入和升级是同一条命令**。

**为什么 `trellis update -s`**：`-s` = skip all modified。目标仓库里被判为 modified 的正是我们
那 8 个自定义文件，跳过它们、只更新 scripts / hooks 恰好是我们要的，而且非交互，可脚本化。
`trellis workflow ... -f` 同理：workflow.md 一定是 modified，`-f` 强制换成新版，
这正是升级的意图。

**顺序不能反**：`trellis update` 先把 scripts / hooks（解析器）升到新版，再让新语法的
workflow.md 落地。反了会出现新语法配旧解析器。脚本内建这个顺序，用户不必记。

**单一写入者原则仍然成立**：脚本不自己写 workflow.md，而是调用官方的
`trellis init` / `trellis workflow` 去写。脚本只做编排。

- **前置校验**（任一失败即退出，非零码）：`trellis` CLI 在 PATH 上；
  `<target>` 是 git 仓库；`.trellis/` 存在时其 `.version` ≥ 0.6.14
- **产物 8 项**：3 个 `.claude/agents/` + 2 个 `.trellis/agents/` + 2 个 `docs/agents/`
  + 1 个 `.claude/commands/trellis/continue.md`

  > 第 8 项是实施中途才发现要加的：`dist/types/ai-tools.js` 给 Claude Code 的
  > `cliFlag` 是 `"claude"`，而解析器只认 `claude-code`／`Claude Code`，导致
  > `/trellis:continue` 里每个平台限定的 step 正文被静默丢弃（实测 4 行 vs 18 行）。
  > 上游 0.6.14 未修。**这一项超出了最初的规格**，但不带它每个消费仓库都会静默劣化，
  > 所以选择扩大规格而不是丢掉修复。仅 Claude Code 需要——其他平台的 cliFlag
  > 与块名模糊匹配后相等。
- **AGENTS.md 幂等注入**：段落用 marker 包裹，按 marker 整段替换；
  marker 不存在则追加到文件末尾（保证落在 `<!-- TRELLIS -->` 托管块之外）

```
<!-- MATTPOCOCK-GRAFT:START v=1.0.0 -->
## Agent skills
...
<!-- MATTPOCOCK-GRAFT:END -->
```

- **结束时打印**：本次是三场景中的哪一种、写了哪些文件、marker 版本从什么变成什么，
  以及必须手动做的两件事——全局装 mattpocock 技能、开新会话让 agent 定义生效
- `--dry-run` 列出将执行的命令与将写入的路径，不落盘

### `upgrade.sh` —— graft 仓库侧的唯一命令

```bash
./upgrade.sh                 # 跟进到 npm latest
./upgrade.sh 0.7.0-beta.3    # 或指定版本
./upgrade.sh --continue      # 人工解完冲突后接着走
```

一条命令按顺序做完全部五步，中间不需要用户敲第二条命令：

1. `npm i -g @mindfoldhq/trellis@<ver>` 升级全局 CLI
2. `npm pack @mindfoldhq/trellis@<ver>` 解出 `dist/templates/`，取得 THEIRS（无需安装即可取任意版本）
3. 对 8 个文件逐个 `git merge-file --diff3`：BASE=`upstream/`，OURS=仓库内我方版本，THEIRS=新原版
4. 跑内建校验：13 个 step 渲染非空 + workflow-state 标签配对 + 平台标记配对
5. 全绿则把 `upstream/` 刷成新 BASE、`upstream/VERSION` 写新版本号、`VERSION` 递增小版本，
   并打印待执行的 `git tag` / `git push` 命令（脚本不自己推，git 写操作留给人）

**有冲突时**：停在第 3 步，列出冲突文件与所在区块（形如 `workflow.md #### 2.2`），
不继续往下走。人工解完冲突标记后跑 `./upgrade.sh --continue`，从第 4 步接着跑。

校验逻辑内联在 `upgrade.sh` 里，不再单独出 `validate.sh`——它只被这一处调用，
单独成文件是没必要的间接层。

## 上游更新的跟进机制

### 上游原版从哪来

`npm pack @mindfoldhq/trellis@<version>` **无需安装**即可取得任意版本的 `dist/templates/`，
该目录就是上游原版的权威副本——已验证与 stock 仓库（ulog 0.6.10）逐字节一致。
8 个受影响文件的模板路径固定：

| 我方文件 | 上游模板路径 |
| --- | --- |
| `.trellis/workflow.md` | `dist/templates/trellis/workflow.md` |
| `.trellis/agents/{check,implement}.md` | `dist/templates/trellis/agents/` |
| `.claude/agents/trellis-{implement,check,research}.md` | `dist/templates/claude/agents/` |
| `.claude/commands/trellis/continue.md` | `dist/templates/common/commands/continue.md` |

### 实测的上游改动频率

跨 7 个版本、约 3.5 周（0.6.7 发布于 2026-07-13，0.6.14 发布于 2026-08-06）：

| 文件 | 0.6.7→0.6.14 | 0.6.10→0.6.14 | 0.6.12→0.6.14 |
| --- | --- | --- | --- |
| `trellis/workflow.md` | 39 行 | 0 | 0 |
| 其余 6 个 | 0 | 0 | 0 |

维护负担很低。绝大多数版本对我们这 8 个文件是零改动，`trellis update` 只更新
scripts / hooks（那些是 auto-update，不冲突）。

### 三方合并（已实测）

`upstream/` 存的是 graft 所基于的上游原版，即合并的 BASE：

```bash
git merge-file --diff3 -L ours -L base -L upstream \
  <我们的版本> upstream/<对应文件> <新版上游原版>
```

**实测结果**：把 0.6.7→0.6.14 那 39 行上游变更压到当前 graft 上，产生 **4 个冲突**，
全部落在我们改写过的 `#### 1.2` / `#### 2.1` / `#### 2.2` 三个 step 正文内；
结构区（`## Phase Index`、7 对 `[workflow-state:*]` 块、平台标记）**零冲突**，
合并后标签配对仍为 OK。冲突只出现在我们动过的地方，这是最好的形状。

### 各仓库怎么跟进

上游合并完、graft 打完 tag 之后，各消费仓库只需一条 `install.sh`——
它内部按 `trellis update -s` → `trellis workflow -f` → 复制 8 项的顺序执行。

**关键顺序**：先 `trellis update` 再拉 graft。scripts / hooks 是解析器，必须先到新版，
再让新版 workflow.md 落地，否则可能出现新语法配旧解析器。这个顺序已内建在脚本里。

## 验证策略

真实命令，不靠检视。全部在 scratchpad 的一次性空仓库中进行，**不碰任何真实仓库**：

```bash
# 1. 解析器契约（官方推荐的唯一权威检查）
for s in 1.0 1.1 1.2 1.3 1.4 1.5 2.1 2.2 2.3 3.2 3.3 3.4 3.5; do
  python3 ./.trellis/scripts/get_context.py --mode phase --step $s --platform claude-code
done

# 2. marketplace 可达
trellis workflow --list -m gh:Episkey-G/trellis-graft

# 3. 端到端（一次性空仓库）
trellis init --claude --workflow trellis-mattpocock \
  --workflow-source gh:Episkey-G/trellis-graft
./install.sh --target <tmp-repo>
./install.sh --target <tmp-repo>        # 第二遍验幂等
```

**会话缓存**：agent 与 skill 定义在会话启动时缓存。第 9 条验收（`trellis-check` 工具集为
4 个只读工具）必须在新会话中确认——上一轮端到端验证正是栽在这里，跑的是旧 agent 定义。

## 兼容性与回滚

| 变更 | `trellis update` 行为 | 回滚 |
| --- | --- | --- |
| `.trellis/workflow.md` | 已在 modified 列表 | git |
| `.trellis/agents/*.md` | 改后进入 modified 列表（+2 项决策） | git 或 `trellis update -f` 还原 native |
| `.claude/skills/implement/SKILL.md` | 非 template-managed，不受影响 | git |
| `AGENTS.md` | 段落在托管块外，判定 unchanged | 按 marker 删段 |
| marketplace 仓库 | 无 | 删仓库或回退 tag；各仓库 `trellis workflow -t native` 回 native |

新增 2 项 `trellis update` 决策（channel agents）是明确代价：换来两套 agent 语义一致。
决策项从 6 个增至 8 个。

## 风险

1. **AGPL-3.0 传染**。workflow.md 与 5 个 agent 定义均为 Trellis 衍生物。公开仓库带
   AGPL-3.0 LICENSE + NOTICE 署名即合规；mattpocock/skills 不在分发范围内（仍由
   `npx skills` 从上游装），不涉及其许可。
2. **channel 只读是软约束**。无 `tools` 字段，只能靠指令。若将来实际启用 channel，
   需要重新验证 worker 是否遵守。本任务不启用，风险被推迟而非消除——写入 3.3 spec。
3. **公开仓库是对外操作**，创建前单独确认。

## README 必须写清的内容

README 是这个仓库唯一的用户界面，三类读者各要一节：

1. **新仓库怎么接** —— 单条 `trellis init --claude --workflow trellis-mattpocock
   --workflow-source gh:Episkey-G/trellis-graft`，以及等价的 `install.sh --target .`
2. **已有仓库怎么接 / 怎么升级** —— 强调这两件事是同一条命令 `install.sh --target <repo>`；
   逐条解释它内部会跑什么（`trellis update -s` → `trellis workflow -f` → 复制 8 项）、
   为什么是这个顺序、`-s` 和 `-f` 各自意味着什么，以及跑完必须手动做的两件事
   （全局装 mattpocock 技能、开新会话）
3. **上游发新版了怎么办** —— 单条 `./upgrade.sh`，冲突时怎么读输出、怎么 `--continue`；
   附上实测的改动频率表，让人知道多数版本是零冲突

每节都要给可直接复制的命令块，不要只描述。
