// NSH 界面根组件。
//
// 注意：nsh.dev 尚未发布可用的 nsh / nsh-macro / nsh-build 包（见 crates/desktop-ui），
// 这里的 TSX 只描述「列表 + 详情 + 工具栏 + 状态栏」的组件结构与数据流，
// 接入官方 NSH SDK 后按其组件 API 调整标签与 hooks 名称即可。
//
// 数据来源：desktop 的 App 状态（requests / settings / status_line / last_exchange）。

import { DetailPanel } from "./components/DetailPanel";
import { RequestList } from "./components/RequestList";
import { StatusBar } from "./components/StatusBar";
import { Toolbar } from "./components/Toolbar";

export interface RequestSummary {
  id: string;
  scheme: "http" | "https";
  method: string;
  host: string;
  path: string;
  url: string;
  status: number | null;
  duration_ms: number;
  request_bytes: number;
  response_bytes: number;
  completed: boolean;
  error: string | null;
}

export interface AppState {
  requests: RequestSummary[];
  selectedId: string | null;
  filter: {
    query: string;
    method: string | null;
    onlyErrors: boolean;
    onlyHttps: boolean;
  };
  engineRunning: boolean;
  listenAddr: string | null;
  statusLine: string;
}

export default function App({ state }: { state: AppState }) {
  const selected = state.requests.find((item) => item.id === state.selectedId) ?? null;

  return (
    <div class="flex h-screen flex-col bg-theme-background text-theme-foreground">
      <Toolbar
        filter={state.filter}
        running={state.engineRunning}
        listenAddr={state.listenAddr}
      />

      <main class="flex min-h-0 flex-1">
        <section class="w-1/2 min-w-[420px] border-r border-theme-border">
          <RequestList
            requests={state.requests}
            selectedId={state.selectedId}
            filter={state.filter}
          />
        </section>

        <section class="flex-1 overflow-hidden">
          <DetailPanel request={selected} />
        </section>
      </main>

      <StatusBar text={state.statusLine} />
    </div>
  );
}
