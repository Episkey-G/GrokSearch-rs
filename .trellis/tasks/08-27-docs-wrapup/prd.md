# 10 文档收口

## Parent

`.trellis/tasks/08-26-issue-triage-batch` — 见其 `prd.md` 与 `design.md`。

## What to build

把这一批的行为变化和一个长期存在的文档缺口写进文档。

**长期缺口**：文档说了普通网址会回落到 source chain，但从没明说「不配任何 source provider 时，非 specialist 站点的抓取会直接失败」。用户是在调用失败之后才发现这件事的。

**本批次的变化**：新的默认时间预算、多 key 分隔符与告警、stdio 并发上限。

**术语一致性**：文档必须与 `CONTEXT.md` 一致地区分 source provider（需要 key 的外部服务）与 specialist extractor（免 key 的站点解析器）。这是 08 那句错误文案的同源问题——文案改了、文档没改，用户还是会得出同样的错误结论。

## Acceptance criteria

- [ ] 文档明确说明：未配置任何 source provider 时，直接抓取工具对 GitHub、StackExchange、arXiv、Wikipedia 之外的普通网址会失败
- [ ] 文档中 source provider 与 specialist extractor 的用法与 `CONTEXT.md` 一致
- [ ] 新的默认时间预算在文档与配置模板中同步
- [ ] 多 key 的分隔符支持与告警行为写入文档
- [ ] stdio 并发上限写入文档
- [ ] 变更日志涵盖本批次全部用户可见变化
- [ ] 公共契约检查脚本通过

## Blocked by

- `08-27-stdio-concurrency`（02）— 并发上限的最终取值
- `08-27-keyring-delimiters`（04）— 分隔符与告警的最终行为
- `08-27-upstream-retry-budget`（06）— 默认时间预算的最终取值
- `08-27-doctor-config-visibility`（07）— 诊断输出的最终字段
- `08-27-empty-chain-message`（08）— 失败说明的最终措辞
- `08-27-tool-call-classification`（09）— 失败分类的最终命名

## Notes

本票合入后发布 0.1.26。02 已随 0.1.25 单独发布。
