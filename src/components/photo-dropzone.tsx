import { useId, useRef } from 'react'
import { LoaderCircleIcon, UploadIcon, XIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

/**
 * 写真を受け取る領域。`<label>` で入力を包んであるので、
 * クリックでもタップでもファイル選択が開く。ドラッグ&ドロップはページ全体が受ける。
 * 選んだあとはこの領域自体が、ファイル名とリセットの行と、選んだ写真の表示になる。そのまま次の写真も受け取れる。
 * 読み取り中は選び直しもリセットもできない (読み取り中の表示を見せたいので、disabled と違って薄くはしない)
 */
export function PhotoDropzone({
  onSelect,
  onReset,
  selected,
  reading,
  disabled,
  showDropHint = true,
}: {
  onSelect: (file: File) => void
  onReset: () => void
  /** 選んだ写真。未選択なら undefined */
  selected?: { url: string; name: string; detail: string }
  reading?: boolean
  disabled?: boolean
  /** ドラッグ&ドロップの案内を出すか (タッチだけの端末では出さない) */
  showDropHint?: boolean
}) {
  const inputId = useId()
  const locked = disabled || reading
  const inputRef = useRef<HTMLInputElement>(null)
  const take = (files: FileList | null) => {
    const file = files?.[0]
    if (file) onSelect(file)
    // 同じ写真を選び直せるようにする
    if (inputRef.current) inputRef.current.value = ''
  }

  return (
    <label
      htmlFor={inputId}
      className={cn(
        'flex w-full rounded-xl bg-card transition-colors',
        !locked && 'cursor-pointer hover:bg-muted/50',
        selected
          ? 'flex-col gap-3 border p-3 text-left'
          : 'flex-col items-center justify-center gap-3 border-2 border-dashed px-6 py-10 text-center',
        disabled && 'pointer-events-none opacity-50',
      )}
    >
      {selected ? (
        <>
          <div className="flex items-center gap-3 px-1">
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium">{selected.name}</p>
              <p className="flex items-center gap-1.5 truncate text-sm text-muted-foreground">
                {reading && <LoaderCircleIcon className="size-3.5 animate-spin text-primary" />}
                {reading ? '読み取り中…' : selected.detail}
              </p>
            </div>
            <Button
              variant="ghost"
              size="sm"
              // label の中なので、押すとファイル選択まで開いてしまう
              onClick={(e) => {
                e.preventDefault()
                onReset()
              }}
              disabled={locked}
            >
              <XIcon />
              リセット
            </Button>
          </div>
          {/* 選んだ写真そのもの (読み取り中でなければ、押すと別の写真を選び直せる)。読み取り中は上に幕をかけて、動いていることを示す。
              古いスマホでは大きな写真が出るまで時間がかかり、その間は写真の大きさが 0 なので、枠に最低限の大きさを持たせる */}
          <div className="relative mx-auto flex min-h-48 w-fit min-w-64 items-center justify-center rounded-md bg-muted">
            {/* 大きな写真を出す準備を待たずに、先に読み取り中の幕を描かせる */}
            <img src={selected.url} alt="選んだ写真" decoding="async" className="max-h-[40vh] w-auto rounded-md" />
            {reading && (
              <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 rounded-md bg-background/75 backdrop-blur-[2px]">
                <LoaderCircleIcon className="size-9 animate-spin text-primary" />
                <p className="text-sm font-medium whitespace-nowrap">護石を読み取っています…</p>
                <div className="h-1 w-40 overflow-hidden rounded-full bg-muted">
                  <div className="h-full w-1/3 animate-indeterminate rounded-full bg-primary" />
                </div>
              </div>
            )}
          </div>
        </>
      ) : (
        <>
          <div className="flex size-12 items-center justify-center rounded-full bg-muted">
            <UploadIcon className="size-6 text-muted-foreground" />
          </div>
          <div className="space-y-1">
            <p className="text-sm font-medium">
              {showDropHint ? '写真をドラッグ&ドロップ、またはクリックして選択' : 'タップして写真を選択'}
            </p>
            <p className="text-sm text-muted-foreground">鑑定画面の写真 (JPEG・PNG・HEIC)</p>
          </div>
        </>
      )}
      <input
        id={inputId}
        ref={inputRef}
        type="file"
        accept="image/*,.heic,.heif"
        className="sr-only"
        disabled={locked}
        onChange={(e) => take(e.target.files)}
      />
    </label>
  )
}
