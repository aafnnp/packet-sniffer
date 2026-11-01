// 顶部工具栏：启停、过滤、清空、安装证书、系统代理开关。
//
// 动作通过 NSH 的事件通道发回 Rust 侧，由 desktop 调用 proxy-engine / system-bridge 执行，
// 界面层不直接持有引擎句柄。

import type { AppState } from "../app";

export type ToolbarAction =
  | { type: "start" }
  | { type: "stop" }
  | { type: "clear" }
  | { type: "install-ca" }
  | { type: "toggle-system-proxy"; enabled: boolean }
  | { type: "update-filter"; filter: AppState["filter"] };

export function Toolbar({
  filter,
  running,
  listenAddr,
  onAction,
}: {
  filter: AppState["filter"];
  running: boolean;
  listenAddr: string | null;
  onAction?: (action: ToolbarAction) => void;
}) {
  const emit = (action: ToolbarAction) => onAction?.(action);

  return (
    <header class="flex items-center gap-2 border-b border-theme-border bg-theme-surface px-3 py-2 text-sm">
      <button
        class="rounded px-3 py-1 " +
        (running ? "bg-rose-500/20 text-rose-300" : "bg-emerald-500/20 text-emerald-300")
        onClick={() => emit({ type: running ? "stop" : "start" })}
      >
        {running ? "停止" : "开始"}
      </button>

      <input
        class="w-64 rounded border border-theme-border bg-theme-background px-2 py-1 font-mono text-xs"
        placeholder="搜索 URL / Host / 方法"
        value={filter.query}
        onInput={(event: { target: { value: string } }) =>
          emit({ type: "update-filter", filter: { ...filter, query: event.target.value } })
        }
      />

      <label class="flex items-center gap-1 text-xs text-theme-muted">
        <input
          type="checkbox"
          checked={filter.onlyHttps}
          onChange={() => emit({ type: "update-filter", filter: { ...filter, onlyHttps: !filter.onlyHttps } })}
        />
        仅 HTTPS
      </label>

      <label class="flex items-center gap-1 text-xs text-theme-muted">
        <input
          type="checkbox"
          checked={filter.onlyErrors}
          onChange={() => emit({ type: "update-filter", filter: { ...filter, onlyErrors: !filter.onlyErrors } })}
        />
        仅异常
      </label>

      <span class="ml-auto font-mono text-xs text-theme-muted">{listenAddr ?? "未监听"}</span>

      <button class="rounded px-2 py-1 hover:bg-theme-hover" onClick={() => emit({ type: "install-ca" })}>
        安装根证书
      </button>
      <button class="rounded px-2 py-1 hover:bg-theme-hover" onClick={() => emit({ type: "toggle-system-proxy", enabled: true })}>
        接管系统代理
      </button>
      <button class="rounded px-2 py-1 hover:bg-theme-hover" onClick={() => emit({ type: "clear" })}>
        清空
      </button>
    </header>
  );
}
