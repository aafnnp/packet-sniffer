// 详情面板：请求 / 响应首部与 body（body 由 Rust 侧 decompress 模块按需解压后传入）。
//
// 完整记录来自 App.last_exchange（common::types::CapturedExchange），
// 列表摘要只用于列表渲染，避免把 body 放进列表数据里。

import type { RequestSummary } from "../app";

export interface CapturedDetail {
  request: {
    method: string;
    url: string;
    version: string;
    headers: Array<{ name: string; value: string }>;
    body: string;
    bodyTruncated: boolean;
  };
  response: {
    status: number;
    statusText: string;
    version: string;
    headers: Array<{ name: string; value: string }>;
    body: string;
    bodyTruncated: boolean;
  } | null;
  error: string | null;
}

function HeaderTable({ headers }: { headers: Array<{ name: string; value: string }> }) {
  return (
    <table class="w-full text-xs font-mono">
      <tbody>
        {headers.map((header) => (
          <tr key={header.name + header.value}>
            <td class="w-48 pr-2 align-top text-theme-muted">{header.name}</td>
            <td class="break-all">{header.value}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function DetailPanel({
  request,
  detail,
}: {
  request: RequestSummary | null;
  detail?: CapturedDetail | null;
}) {
  if (!request) {
    return (
      <div class="flex h-full items-center justify-center text-sm text-theme-muted">
        选择左侧请求查看详情
      </div>
    );
  }

  return (
    <div class="h-full overflow-y-auto p-4">
      <h2 class="mb-3 font-mono text-sm">
        <span class="text-theme-muted">{request.method}</span> {request.url}
      </h2>

      {detail?.error && (
        <div class="mb-3 rounded border border-rose-500/40 bg-rose-500/10 p-2 text-xs text-rose-300">
          {detail.error}
        </div>
      )}

      <h3 class="mb-1 text-xs uppercase text-theme-muted">请求首部</h3>
      <HeaderTable headers={detail?.request.headers ?? []} />

      <h3 class="mt-4 mb-1 text-xs uppercase text-theme-muted">
        请求体{detail?.request.bodyTruncated ? "（已截断）" : ""}
      </h3>
      <pre class="overflow-x-auto rounded bg-theme-surface p-2 text-xs">
        {detail?.request.body || "（空）"}
      </pre>

      <h3 class="mt-4 mb-1 text-xs uppercase text-theme-muted">响应首部</h3>
      <HeaderTable headers={detail?.response?.headers ?? []} />

      <h3 class="mt-4 mb-1 text-xs uppercase text-theme-muted">
        响应体{detail?.response?.bodyTruncated ? "（已截断）" : ""}
      </h3>
      <pre class="overflow-x-auto rounded bg-theme-surface p-2 text-xs">
        {detail?.response?.body || "（空）"}
      </pre>
    </div>
  );
}
