# 03 key ring 共享化

## Parent

`.trellis/tasks/08-26-issue-triage-batch` — 见其 `prd.md` 与 `design.md`。

## What to build

key ring（单个上游的多凭证轮换环）目前私有于 Tavily provider。把它提取为共享构件，让 Tavily、Grok Responses、OpenAI 兼容三方复用同一份实现与同一套轮换判定。

**纯搬迁，零行为变化。** Tavily 的轮换行为必须逐字节保持不变。这张票的价值是让 04（分隔符健壮性）和 05（AI 上游轮换）能各自独立开工，而不是排成一条链。

## Acceptance criteria

- [ ] key ring 及其 key-scoped 状态码判定位于共享位置，可被多个 provider 复用
- [ ] Tavily 现有轮换测试全绿，且断言未被修改
- [ ] key-scoped 状态码集合（401/403/429 及各上游配额码）与提取前完全一致
- [ ] 轮换游标的共享语义不变：provider 的多个克隆共用同一个游标
- [ ] 随机起始偏移的行为不变
- [ ] 无新增配置项，无用户可见行为变化

## Blocked by

None — can start immediately.
