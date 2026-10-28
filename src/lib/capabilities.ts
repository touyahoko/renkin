/** ブラウザが読み取りと検索に必要な機能を備えているか */
export type Capabilities = {
  /** 使い始められるか */
  ready: boolean
  /** 足りないものの説明。ready が true なら空 */
  missing: string[]
}

/** SIMD 命令 (i8x16.splat, i8x16.popcnt) を 1 つ含むだけの wasm (wasm-feature-detect の simd と同じ)。
 * 読み取りの wasm は simd128 を使ってビルドしているので、これが通らないブラウザでは読み込めない */
const SIMD_PROBE = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1, 8, 0, 65, 0, 253, 15, 253, 98, 11,
])

export function detectCapabilities(): Capabilities {
  const missing: string[] = []
  if (typeof WebAssembly !== 'object') {
    missing.push('WebAssembly に対応していません')
  } else if (!WebAssembly.validate(SIMD_PROBE)) {
    // Chrome 91 / Firefox 89 / Safari 16.4 から。iPhone・iPad はどのブラウザも Safari と同じエンジンで、iOS と一緒にしか
    // 更新されないので、iOS 16.4 以降が要る
    missing.push('WebAssembly の SIMD に対応していません。ブラウザを最新にしてください (iPhone・iPad は iOS 16.4 以降が必要です)')
  }
  if (typeof Worker === 'undefined') {
    missing.push('Web Worker に対応していません')
  }
  if (typeof OffscreenCanvas === 'undefined' || typeof createImageBitmap === 'undefined') {
    missing.push('画像の処理 (OffscreenCanvas) に対応していません')
  }
  return { ready: missing.length === 0, missing }
}
