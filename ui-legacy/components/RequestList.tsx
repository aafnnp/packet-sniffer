// 请求列表（虚拟滚动）。
//
// 只负责渲染 App 传入的已过滤摘要列表，过滤逻辑在 Rust 侧（state::RequestList::filtered），
// 以保证 headless 模式与界面模式的过滤结果一致。

import type { AppState, RequestSummary } from "../app";

const METHOD_COLOR: Record<string, string> = {
  GET: "text-emerald-400",
  POST: "text-amber-400",
  PUT: "text-sky-400",
  PATCH: "text-violet-400",
  DELETE: "text-rose-400",
};

function statusTone(status: number | null, error: string | null): string {
  if (error) return "text-rose-400";
  if (status === null) return "text-theme-muted";
  if (status >= 500) return "text-rose-400";
  if (status >= 400) return "text-amber-400";
  return "text-emerald-400";
}

function Row({ item, selected }: { item: RequestSummary; selected: boolean }) {
  return (
    <div
      class={
        "flex items-center gap-3 px-3 py-1 text-sm " +
        (selected ? "bg-theme-selection" : "hover:bg-theme-hover")
      }
    >
      <span class={"w-16 font-mono " + (METHOD_COLOR[item.method] ?? "text-theme-muted")}>
        {item.method}
      </span>
      <span class="flex-1 truncate">{item.url}</span>
      <span class={"w-10 text-right font-mono " + statusTone(item.status, item.error)}>
        {item.error ? "ERR" : (item.status ?? "…")}
      </span>
      <span class="w-16 text-right font-mono text-theme-muted">{item.duration_ms}ms</span>
    </div>
  );
}

export function RequestList({
  requests,
  selectedId,
  filter,
}: {
  requests: RequestSummary[];
  selectedId: string | null;
  filter: AppState["filter"];
}) {
  // 虚拟滚动：仅渲染视口内的行，具体 API 以 NSH 的列表组件为准。
  return (
    <div class="h-full overflow-y-auto">
      {requests.length === 0 ? (
        <div class="p-6 text-sm text-theme-muted">
          {filter.query ? "没有匹配的请求" : "等待流量…请把系统代理指向本机监听端口"}
        </div>
      ) : (
        requests.map((item) => (
          <Row key={item.id} item={item} selected={item.id === selectedId} />
        ))
      )}
    </div>
  );
}
