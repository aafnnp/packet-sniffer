// 设置页：对应 desktop 的 state::Settings（持久化到 ~/.packet-sniffer/settings.json）。

export interface SettingsView {
  listenAddr: string;
  captureEnabled: boolean;
  maxBodyBytes: number;
  storeCapacity: number;
  caDir: string;
  systemProxy: boolean;
  theme: "system" | "light" | "dark";
}

export function Settings({ value, onChange }: { value: SettingsView; onChange?: (next: SettingsView) => void }) {
  return (
    <form class="space-y-4 p-6 text-sm">
      <h1 class="text-base font-semibold">设置</h1>

      <label class="block">
        <span class="mb-1 block text-theme-muted">监听地址</span>
        <input
          class="w-64 rounded border border-theme-border bg-theme-background px-2 py-1 font-mono"
          value={value.listenAddr}
          onInput={(event: { target: { value: string } }) =>
            onChange?.({ ...value, listenAddr: event.target.value })
          }
        />
      </label>

      <label class="flex items-center gap-2">
        <input
          type="checkbox"
          checked={value.captureEnabled}
          onChange={() => onChange?.({ ...value, captureEnabled: !value.captureEnabled })}
        />
        解密 HTTPS（关闭后只做透明转发）
      </label>

      <label class="block">
        <span class="mb-1 block text-theme-muted">单个 body 抓取上限（字节）</span>
        <input
          type="number"
          class="w-40 rounded border border-theme-border bg-theme-background px-2 py-1 font-mono"
          value={value.maxBodyBytes}
          onInput={(event: { target: { value: string } }) =>
            onChange?.({ ...value, maxBodyBytes: Number(event.target.value) })
          }
        />
      </label>

      <label class="block">
        <span class="mb-1 block text-theme-muted">内存存储条数上限</span>
        <input
          type="number"
          class="w-40 rounded border border-theme-border bg-theme-background px-2 py-1 font-mono"
          value={value.storeCapacity}
          onInput={(event: { target: { value: string } }) =>
            onChange?.({ ...value, storeCapacity: Number(event.target.value) })
          }
        />
      </label>

      <label class="block">
        <span class="mb-1 block text-theme-muted">根证书目录</span>
        <input
          class="w-96 rounded border border-theme-border bg-theme-background px-2 py-1 font-mono"
          value={value.caDir}
          onInput={(event: { target: { value: string } }) =>
            onChange?.({ ...value, caDir: event.target.value })
          }
        />
      </label>

      <label class="flex items-center gap-2">
        <input
          type="checkbox"
          checked={value.systemProxy}
          onChange={() => onChange?.({ ...value, systemProxy: !value.systemProxy })}
        />
        启动时接管系统代理
      </label>

      <label class="block">
        <span class="mb-1 block text-theme-muted">主题</span>
        <select
          class="rounded border border-theme-border bg-theme-background px-2 py-1"
          value={value.theme}
          onChange={(event: { target: { value: SettingsView["theme"] } }) =>
            onChange?.({ ...value, theme: event.target.value })
          }
        >
          <option value="system">跟随系统</option>
          <option value="light">浅色</option>
          <option value="dark">深色</option>
        </select>
      </label>

      <p class="text-xs text-theme-muted">
        当前引擎状态：{/* 由 StatusBar 统一展示 */}见底部状态栏
      </p>
    </form>
  );
}
