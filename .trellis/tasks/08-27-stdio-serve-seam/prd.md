# 01 stdio 服务循环接缝化

## Parent

`.trellis/tasks/08-26-issue-triage-batch` — 见其 `prd.md` 与 `design.md`。

## What to build

stdio transport 的请求服务循环目前和进程的真实 stdin/stdout 焊死，调度逻辑没有任何可测接缝。把它拆成两层：一个只负责绑定真实 stdio 的薄壳，和一个接受任意 reader/writer 的服务循环。

**这张票不改变任何用户可见行为**——处理仍然是逐条串行的。它交付的是「能测」，让下一张票（并发化）的改动小到可以被单独审查和单独 revert。这是这批修复里唯一改动所有用户热路径的地方，值得先把安全网架好。

同时在新接缝上补齐刻画测试，锁住现有行为，作为并发化的回归基线。

## Acceptance criteria

- [ ] 服务循环可以用内存双工管道驱动，不依赖进程 stdin/stdout
- [ ] 真实 stdio 入口只剩绑定职责，调度逻辑全部位于可测核心中
- [ ] 刻画测试覆盖 `initialize` 版本协商，包含客户端请求已知版本时原样回应、请求未知版本时回落到最新版本两种情况
- [ ] 刻画测试覆盖：无 `id` 的通知不产生任何响应
- [ ] 刻画测试覆盖：非法 JSON 返回 -32700，且不中断后续请求处理
- [ ] 刻画测试覆盖：空行被跳过
- [ ] 现有测试全绿，用户可见行为零变化

## Blocked by

None — can start immediately.
