import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { approximateTime, parseFrame } from '@/lib/frame'

/** 錬金したフレームを探す範囲 [開始, 終了) */
export function SearchRangeFields({
  start,
  end,
  onStartChange,
  onEndChange,
  disabled,
}: {
  start: string
  end: string
  onStartChange: (value: string) => void
  onEndChange: (value: string) => void
  disabled?: boolean
}) {
  const a = parseFrame(start)
  const b = parseFrame(end)
  const ordered = a !== null && b !== null && b > a
  return (
    <div className="space-y-2">
      {/* 上の「読み取った画面 | 護石」と同じ 5:6 の列にして、終了フレームの欄を護石の欄と同じ位置から始める */}
      <div className="grid gap-3 sm:grid-cols-2 md:grid-cols-[5fr_6fr] md:gap-x-6">
        <div className="space-y-1.5">
          <Label className="text-sm font-normal text-muted-foreground" htmlFor="frame-start">
            開始フレーム
          </Label>
          <Input
            id="frame-start"
            inputMode="numeric"
            className="font-mono"
            value={start}
            disabled={disabled}
            aria-invalid={a === null}
            onChange={(e) => onStartChange(e.target.value)}
          />
        </div>
        <div className="space-y-1.5 md:pl-6">
          <Label className="text-sm font-normal text-muted-foreground" htmlFor="frame-end">
            終了フレーム
          </Label>
          <Input
            id="frame-end"
            inputMode="numeric"
            className="font-mono"
            value={end}
            disabled={disabled}
            aria-invalid={b === null || !ordered}
            onChange={(e) => onEndChange(e.target.value)}
          />
        </div>
      </div>
      {ordered ? (
        <p className="text-sm text-muted-foreground">
          ゲーム開始から {approximateTime(a)} 〜 {approximateTime(b)}
        </p>
      ) : (
        <p className="text-sm text-destructive">{a === null || b === null ? '0 以上の整数' : '終了は開始より後'}</p>
      )}
    </div>
  )
}
