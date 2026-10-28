/** マウスなど細かく指せる入力があるか。タッチだけの端末ではドラッグ&ドロップの案内を出さない */
export function hasFinePointer(): boolean {
  return window.matchMedia?.('(pointer: fine)').matches ?? false
}

/** バイト数を読みやすくする */
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}
