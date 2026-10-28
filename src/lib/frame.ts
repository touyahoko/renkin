/** 1 秒のフレーム数 (mhxx-rng と同じ) */
export const FPS = 30

/** フレームをゲーム開始からの時間にする (「3時間 32分 58秒 6F」。0 の上の単位は省く) */
export function frameToTime(frame: number): string {
  const f = frame % FPS
  let s = Math.floor(frame / FPS)
  const units: [number, string][] = []
  for (const [size, unit] of [
    [86400, '日'],
    [3600, '時間'],
    [60, '分'],
  ] as const) {
    units.push([Math.floor(s / size), unit])
    s %= size
  }
  units.push([s, '秒'])
  const first = units.findIndex(([v]) => v > 0)
  const shown = first < 0 ? [] : units.slice(first)
  return [...shown.map(([v, u]) => `${v}${u}`), `${f}F`].join(' ')
}

/** フレームの番号として読めれば数、読めなければ null (0 以上の整数、u32 の範囲) */
export function parseFrame(text: string): number | null {
  const t = text.replaceAll(',', '').trim()
  if (!/^\d+$/.test(t)) return null
  const n = Number(t)
  return n <= 4_294_967_295 ? n : null
}

/** フレームをゲーム開始からの時間としてざっくり表す */
export function approximateTime(frame: number): string {
  if (frame <= 0) return '0秒'
  const seconds = frame / FPS
  if (seconds >= 86400) return `約${Math.round(seconds / 86400).toLocaleString()}日`
  if (seconds >= 3600) return `約${Math.round(seconds / 3600)}時間`
  if (seconds >= 60) return `約${Math.round(seconds / 60)}分`
  return `約${Math.round(seconds)}秒`
}
