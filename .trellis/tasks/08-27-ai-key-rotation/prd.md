# 05 AI 上游多 key 轮换

## Parent

`.trellis/tasks/08-26-issue-triage-batch` — 见其 `prd.md` 与 `design.md`。

## What to build

source provider 早就支持多 key 轮换，AI 上游还是单凭证：用中转网关时限流和认证失败很常见，换 key 只能改配置重启，一次请求里没法自动试下一个。

让 Grok Responses 与 OpenAI 兼容两条 transport 都接受多凭证，遇到 key 范围的失败（认证失败、限流）自动换下一个凭证重发。

**设计要点：扩展凭证抽象，而不是绕过它。** 轮换判定需要看 HTTP 状态码，而 Grok Responses 取凭证走的是只返回 token、不感知状态码的凭证抽象。给该抽象增加「凭证总数」与「取第 N 个凭证」两个能力：静态 API key 实现内部持有 key ring，OAuth 实现回答 1 且忽略序号。轮换循环留在 provider 的请求层——那里才看得到状态码。新增能力带默认实现，现有实现者无需改动。

不选「provider 直接持 key ring、绕过凭证抽象」：那会让 Grok provider 长出两条取凭证路径，OAuth 那条以后每次改动都要同步维护。

## Acceptance criteria

- [ ] Grok Responses 与 OpenAI 兼容两条 transport 的 API key 配置项均接受多个凭证
- [ ] 认证失败与限流（key 范围的状态码）触发轮换到下一个凭证并重发
- [ ] 所有凭证都失败时，返回最后一次尝试的真实错误，而不是笼统的配置缺失
- [ ] 5xx 与超时不触发轮换（那是上游范围的问题，换凭证只增加延迟）
- [ ] OAuth 模式行为完全不变：单凭证，不轮换
- [ ] 日志与错误信息中不出现完整 key
- [ ] 轮换发生时日志能看出第几个凭证、什么状态码、换到下一个
- [ ] 远程 HTTP transport 下，调用方通过请求头自带的多凭证同样生效

## Blocked by

- `08-27-keyring-shared`（03）— 复用共享 key ring，不重复实现轮换

## Notes

覆盖 issue #14 的诉求 1。诉求 2（重试）在 06。诉求 3（多 URL / 多模型）明确不做。
