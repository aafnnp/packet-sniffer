// 规则管理页（第二迭代）。
//
// MVP 阶段只提供前端骨架：断点 / 改写 / 重发规则由 proxy-engine 后续以
// "规则列表 + 匹配后动作" 的形式实现，界面通过事件通道下发。

export interface Rule {
  id: string;
  enabled: boolean;
  /** 匹配表达式，例如 `host:*.example.com && method:POST` */
  matcher: string;
  action: "block" | "breakpoint" | "rewrite" | "delay";
  /** 动作参数，例如改写目标 URL、延迟毫秒数 */
  payload: string;
}

const EXAMPLES: Rule[] = [
  {
    id: "example-block",
    enabled: false,
    matcher: "host:ads.example.com",
    action: "block",
    payload: "",
  },
];

export function Rules({
  rules = EXAMPLES,
  onChange,
}: {
  rules?: Rule[];
  onChange?: (next: Rule[]) => void;
}) {
  const toggle = (id: string) =>
    onChange?.(rules.map((rule) => (rule.id === id ? { ...rule, enabled: !rule.enabled } : rule)));

  return (
    <div class="p-6 text-sm">
      <h1 class="mb-1 text-base font-semibold">规则</h1>
      <p class="mb-4 text-xs text-theme-muted">
        断点、改写、重发规则将在后续迭代接入引擎；当前仅保存规则配置。
      </p>

      <table class="w-full text-left">
        <thead class="text-xs uppercase text-theme-muted">
          <tr>
            <th class="w-16 py-1">启用</th>
            <th class="py-1">匹配</th>
            <th class="w-24 py-1">动作</th>
            <th class="py-1">参数</th>
          </tr>
        </thead>
        <tbody class="font-mono text-xs">
          {rules.map((rule) => (
            <tr key={rule.id} class="border-t border-theme-border">
              <td class="py-1">
                <input type="checkbox" checked={rule.enabled} onChange={() => toggle(rule.id)} />
              </td>
              <td class="py-1">{rule.matcher}</td>
              <td class="py-1">{rule.action}</td>
              <td class="py-1 text-theme-muted">{rule.payload || "-"}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
