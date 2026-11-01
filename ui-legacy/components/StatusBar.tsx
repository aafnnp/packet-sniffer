// 底部状态栏：直接展示 Rust 侧 App::status_line() 的结果，保证两种运行模式信息一致。

export function StatusBar({ text }: { text: string }) {
  return (
    <footer class="flex items-center justify-between border-t border-theme-border bg-theme-surface px-3 py-1 text-xs text-theme-muted">
      <span>{text}</span>
      <span class="font-mono">Rust engine · tokio</span>
    </footer>
  );
}
