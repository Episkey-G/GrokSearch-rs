# 08 空 source chain 的失败说明

## Parent

`.trellis/tasks/08-26-issue-triage-batch` — 见其 `prd.md` 与 `design.md`。
**术语依 `CONTEXT.md`：source provider 与 specialist extractor 是两个东西。**

## What to build

一个 source provider 都没配的用户，搜索结果里每条来源都写着「未匹配到 specialist」。这句话是错的——specialist extractor 是免 key 的站点解析器，跟用户没配 key 毫无关系；真实原因是没有任何 source provider 可以抓取普通网址。

这句文案的实际后果有据可查：一份 bug 报告通篇写「specialist key」，把免 key 的 specialist extractor 和需要 key 的 source provider 混为一谈，并据此得出了错误的性能结论。**是我们的文案教会了用户错误的心智模型。**

source chain 为空时，失败说明要指明「未配置任何 source provider」并列出可选项。

## Acceptance criteria

- [ ] source chain 为空时，失败说明指明未配置任何 source provider，并列出可配置的选项
- [ ] source chain 非空、但确实没有 specialist extractor 匹配该 URL 时，原有措辞与行为完全不变
- [ ] 新措辞不使用 specialist 相关词汇来描述 key 缺失
- [ ] 直接抓取工具与搜索结果内联富化两条路径的措辞一致
- [ ] specialist extractor 命中的 URL（GitHub、StackExchange、arXiv、Wikipedia）在无任何 source provider 时仍然正常工作，不受本改动影响

## Blocked by

None — can start immediately.
