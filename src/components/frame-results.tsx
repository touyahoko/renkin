import { useState } from 'react'
import { AlertTriangleIcon } from 'lucide-react'

import { Alert, AlertTitle, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { frameToTime } from '@/lib/frame'
import type { FrameCandidate } from '@/lib/types'
import { cn } from '@/lib/utils'

/** 一度に出す数。広い範囲で探して候補が多くても畳まずに並べ、続きは「さらに表示」で出す */
const PAGE = 100

/** 錬金時のフレームの候補。フレームを大きく出し、ゲーム開始からの時間を添える */
export function FrameResults({ found, indexKnown }: { found: FrameCandidate[]; indexKnown: boolean }) {
  const [shown, setShown] = useState(PAGE)
  const unique = found.length === 1

  const list = (
    <div className="space-y-3">
      {/* 候補が多いときは詰めて並べる (スマホ 2 列、640px 以上 3 列、1024px 以上 4 列) */}
      <ul className={cn('grid gap-2', !unique && 'grid-cols-2 sm:grid-cols-3 lg:grid-cols-4')}>
        {found.slice(0, shown).map((c) => (
          <li
            key={c.frame}
            className={cn('relative space-y-0.5 rounded-lg border p-3', unique && 'border-primary/40 bg-primary/5')}
          >
            <p
              className={cn(
                'font-mono tabular-nums select-all',
                unique ? 'text-3xl' : 'text-lg sm:text-2xl',
                // 右上の札の分だけ空ける
                !indexKnown && 'pr-14',
              )}
            >
              {c.frame}
            </p>
            <p className="text-xs text-muted-foreground sm:text-sm">{frameToTime(c.frame)}</p>
            {/* ボックス内の位置がわかっていれば全部同じなので出さない。出すときは右上に揃えて置く */}
            {!indexKnown && (
              <div className="absolute top-2.5 right-2.5 flex flex-col items-end gap-1">
                {c.indexes.map((i) => (
                  <Badge key={i} variant="secondary">
                    {i} 番目
                  </Badge>
                ))}
              </div>
            )}
          </li>
        ))}
      </ul>
      {shown < found.length && (
        <Button variant="outline" onClick={() => setShown(shown + PAGE)}>
          さらに表示 (残り {(found.length - shown).toLocaleString()} 件)
        </Button>
      )}
    </div>
  )

  return (
    <Card>
      <CardHeader>
        <CardTitle>錬金時のフレーム</CardTitle>
        {found.length > 0 && (
          <CardDescription>{unique ? '一意に特定' : `候補 ${found.length.toLocaleString()} 件`}</CardDescription>
        )}
      </CardHeader>
      <CardContent>
        {found.length === 0 ? (
          <Alert variant="warning">
            <AlertTriangleIcon />
            <AlertTitle>この探索範囲では見つかりませんでした</AlertTitle>
            <AlertDescription>
              護石の中身と探索範囲を確認してください。
            </AlertDescription>
          </Alert>
        ) : (
          list
        )}
      </CardContent>
    </Card>
  )
}
