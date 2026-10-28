import { useEffect, useRef, useState } from 'react'

/**
 * ページのどこに落としてもファイルを受け取る。
 *
 * 受け口を一部に限ると、外したときにブラウザがそのファイルを開いてページを捨てる。
 * 解析中なら結果も消えるので、`dragover` と `drop` は無効時でも既定動作を止める。
 *
 * 戻り値はファイルをドラッグ中かどうか。呼ぶ側が目印を出すのに使う
 */
export function useWindowFileDrop(onDrop: (file: File) => void, disabled = false): boolean {
  const [dragging, setDragging] = useState(false)
  // dragenter と dragleave は子要素をまたぐたびに飛ぶので、数えないとちらつく
  const depth = useRef(0)
  // 毎描画で作り直される関数を受け取るので、listener を貼り直さずに済むよう持ち替える
  const latest = useRef(onDrop)
  useEffect(() => {
    latest.current = onDrop
  })

  useEffect(() => {
    // ページ内の文字を選んで動かしただけのときに反応しないよう、ファイルだけ見る
    const hasFiles = (e: DragEvent) => e.dataTransfer?.types.includes('Files') ?? false

    const onEnter = (e: DragEvent) => {
      if (!hasFiles(e)) return
      depth.current += 1
      if (!disabled) setDragging(true)
    }
    const onLeave = (e: DragEvent) => {
      if (!hasFiles(e)) return
      depth.current = Math.max(0, depth.current - 1)
      if (depth.current === 0) setDragging(false)
    }
    const onOver = (e: DragEvent) => {
      if (hasFiles(e)) e.preventDefault()
    }
    const onDropped = (e: DragEvent) => {
      if (!hasFiles(e)) return
      e.preventDefault()
      depth.current = 0
      setDragging(false)
      const file = e.dataTransfer?.files?.[0]
      if (file && !disabled) latest.current(file)
    }

    window.addEventListener('dragenter', onEnter)
    window.addEventListener('dragleave', onLeave)
    window.addEventListener('dragover', onOver)
    window.addEventListener('drop', onDropped)
    return () => {
      window.removeEventListener('dragenter', onEnter)
      window.removeEventListener('dragleave', onLeave)
      window.removeEventListener('dragover', onOver)
      window.removeEventListener('drop', onDropped)
    }
  }, [disabled])

  return dragging
}
