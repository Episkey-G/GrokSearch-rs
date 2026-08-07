# Trellis workflow 自定义标准化与多仓库分发

## 问题陈述

groksearch-rs 上已经跑通一套 graft：保留 Trellis 的三阶段状态机、任务树、spec 注入和提交闸门，
把每个 phase 内部的技能层换成 mattpocock/skills。这套东西目前有三个问题：

1. **没改完**。5 处不一致，其中 1 处会真实破坏设计意图——`trellis-check` 同时存在技能和
   agent 两种形态，而 graft 把上游原有的消歧提示删掉了，模型可能去加载那个会自我修复的技能，
   正好绕过只读评审的设计。
2. **没有分发通道**。13 个仓库还在跑 stock Trellis，本仓库的 8 项改动全靠手工复制。
3. **新仓库不会继承**。`trellis init` 拿的是 CLI 内置 native 模板，没有任何用户级默认覆盖机制。

## 方案

三件交付物：

- **D1** 把本仓库补齐到 Trellis 官方定义的标准形式
- **D2** 建公开 marketplace 仓库 `Episkey-G/trellis-graft`，让 workflow.md 走官方通道
- **D3** 在同一仓库里附 `install.sh`，拉齐 workflow.md 之外的 8 项，并在一个干净环境端到端验证

## 用户故事

1. 作为维护者，我在任意新仓库执行一条命令就能初始化出带 graft 的 Trellis 项目，
   不需要事后手工复制 agent 定义。
2. 作为维护者，我修改 `trellis-check.md` 后，推一次 marketplace 仓库，
   其余仓库跑一条命令即可拉齐，不必手工同步 14 份。
3. 作为维护者，我执行 `trellis update` 时，自定义的 workflow.md 和 agent 定义会被判定为
   "Modified by you"，需要我确认，绝不被静默还原成 native 字节。
4. 作为维护者，主会话里不会有被替换掉的旧技能（`trellis-brainstorm`、`trellis-check` 技能形态、
   mattpocock `implement`）意外抢触发。
5. 作为维护者，Phase 2.2 的两个评审 agent 确实没有写权限，findings 只能经由
   `trellis-implement` 落地，不会两个 reviewer 并发写同一棵工作树。
6. 作为维护者，我能一眼看出某个仓库当前跑的是哪个版本的 graft。
7. 作为使用 `trellis channel` 的维护者，channel runtime 的 agent 语义与 `.claude/agents/` 一致，
   不会一套只读一套自修复。
8. 作为维护者，Trellis 官方发新版时，我能机械地把上游变更合进 graft，而不是靠记忆重做一遍
   嫁接——仓库里存着 graft 所基于的上游原版作为三方合并的 BASE，一条脚本完成合并与验证。

## D1 范围：5 处待修（按影响排序）

| # | 问题 | 落点 |
| --- | --- | --- |
| 1 | `trellis-check` 技能/agent 同名，上游消歧提示被删 | `.trellis/workflow.md` `[workflow-state:in_progress]` |
| 2 | channel runtime 的两个 agent 定义未同步 graft 语义 | `.trellis/agents/{check,implement}.md` |
| 3 | inline 平台块 2.1/2.2 正文与 breadcrumb 自相矛盾 | `.trellis/workflow.md` |
| 4 | `prototype` 技能被引用但未安装 | `.claude/skills/` |
| 5 | 被替换的旧技能仍可自动触发 | `AGENTS.md` + `.claude/skills/implement/SKILL.md` |

原清单里的第 6 条（`CONTEXT.md` / `docs/adr/` 不存在）经核实**不是缺陷**：
`docs/agents/domain.md` 明确要求两者缺失时静默通过，由 `domain-modeling` 懒创建。已划除。

## 实现决策

- **留在 Trellis 0.6.14**。`trellis init --workflow <id> --workflow-source <src>` 与
  `trellis workflow -m -t` 在 0.6.14 均已可用，足以满足全部需求。0.7 新增的
  `.trellis/workflows/` 变体库和四层优先级链解决的是"单仓库内多变体切换"，本任务不需要，
  不值得让 14 个活跃仓库承担 beta 风险。
- **marketplace 仓库公开**。内容只有 workflow 模板和 agent 定义，无敏感信息；
  私有会引入 `GIGET_AUTH` 依赖，且无 token 环境下 `--workflow-source` 会静默回退到 native。
- **`install.sh` 不碰 workflow.md**。理由是保持单一写入者，不是技术强制——已核对
  `update.js:662-679`，hash 缺失与 hash 不匹配走同一分支，都进 "Modified by you"，
  脚本写 workflow.md 并不会破坏契约。但 workflow.md 已有官方通道
  （`trellis init --workflow-source` / `trellis workflow -m -t`），两个写入者只会带来歧义。
- **仓库里存 `upstream/` 基线**。上游发新版时靠三方合并（BASE=graft 所基于的上游原版）
  把变更合进来，而不是靠记忆重做嫁接。上游原版可用 `npm pack @mindfoldhq/trellis@<ver>`
  机械获取，无需安装。
- **AGENTS.md 段落用 marker 包裹**，脚本按 marker 幂等替换，且始终位于 `<!-- TRELLIS -->`
  托管块之外——已验证这样 `trellis update` 判定 AGENTS.md 为 unchanged。
- **旧技能抑制分两种手法**：template-managed 的（`trellis-brainstorm`、`trellis-check` 技能）
  只在 AGENTS.md 里声明"已被替换、不要自动加载"，不改文件本身，避免给 `trellis update`
  增加决策项；非 template-managed 的 mattpocock `implement` 直接加
  `disable-model-invocation: true`。
- **`prototype` 装上而不是删引用**。上游 `skills/engineering/prototype` 存在，且已装的
  `to-spec`、`to-tickets` 都假定它在工具链里（"if a prototype produced a snippet…"）。
  装上后技能总数 11 → 12。

## 测试决策

无单元测试可写——产物是 markdown 契约和 shell 脚本。验证走真实命令：

- 官方推荐的解析器验证：`get_context.py --mode phase --step X.Y --platform claude-code`
  对 13 个 step 全部返回非空
- 7 对 `[workflow-state:*]` 标签配对、平台标记配对完好
- `trellis workflow --list -m gh:Episkey-G/trellis-graft` 能列出新模板
- 在 scratchpad 的一次性空仓库里跑完整 init + install，**不碰任何真实仓库**
- `install.sh` 连跑两次产物一致（幂等）
- 新会话中 agent 列表显示 `trellis-check (Tools: Read, Bash, Glob, Grep)`

## 约束

- **AGPL-3.0-only**。Trellis 采用 AGPL-3.0-only，workflow.md 与 agent 定义均为其衍生物。
  marketplace 仓库必须带 AGPL-3.0 LICENSE 并署名 `mindfold-ai/Trellis`。
  mattpocock/skills 不在分发范围内（仍由 `npx skills` 从上游安装），不涉及其许可。
- **agent 与 skill 定义在会话启动时缓存**。任何验证都必须在新会话中进行，
  这正是上一轮端到端验证跑到旧 agent 定义的原因。
- **git 写操作仅在明确要求时执行**，代码改动走分支提 PR，不直推 main。
- **创建公开 GitHub 仓库是对外操作**，执行前需单独确认。

## 不在本任务范围

- 13 个仓库的实际 rollout（另开 task；每个仓库都要开新会话才能让 agent 定义生效，
  失败点分散，不适合与本任务混在一起）
- 升级到 0.7.0-beta
- 填充 `.trellis/spec/backend/*.md` 占位符模板
- 工作树里那约 3500 行未提交的业务改动（setup wizard / doctor CLI / quality gate 等）

## 验收标准

1. 13 个 step 在 `--platform claude-code` 下全部渲染非空，7 对 workflow-state 标签配对
2. `[workflow-state:in_progress]` 含 `trellis-check` 技能/agent 消歧，且 breadcrumb 体积
   不超过上游同块（872B）
3. `.trellis/agents/{check,implement}.md` 语义与 `.claude/agents/` 对应文件一致
4. inline 平台块 2.1/2.2 正文与 `[workflow-state:in_progress-inline]` 一致
5. `.claude/skills/prototype/` 存在；`implement` 技能带 `disable-model-invocation: true`
6. `Episkey-G/trellis-graft` 公开可访问，带 AGPL-3.0 LICENSE 与署名，
   `trellis workflow --list -m gh:Episkey-G/trellis-graft` 列出 `trellis-mattpocock`
7. 一次性空仓库中 `trellis init --claude --workflow trellis-mattpocock --workflow-source
   gh:Episkey-G/trellis-graft` 产出的 workflow.md 与本仓库一致，
   且 `.template-hashes.json` 中无 `.trellis/workflow.md` 条目
8. 同一空仓库中 `install.sh` 跑两次，8 项产物一致
9. 新会话中 `trellis-check` agent 工具集为 `Read, Bash, Glob, Grep`
10. `upstream/` 存有 8 个上游原版且 `upstream/VERSION` 为 `0.6.14`；
    `upgrade.sh` 能对一个已知的上游变更完成三方合并，冲突全部落在 step 正文内，
    结构区（`## Phase Index`、7 对 workflow-state 块、平台标记）零冲突
