import type { ReactNode } from 'react'
import { Dialog as DialogPrimitive } from '@base-ui/react/dialog'
import { ImageIcon, XIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Dialog, DialogClose, DialogOverlay, DialogPortal, DialogTitle, DialogTrigger } from '@/components/ui/dialog'

import { cn } from '@/lib/utils'

/** 位置合わせ後の画面 (1280x720) のうち、護石の欄と鑑定ボックスを含む範囲。画面全体を縮めると文字が読めないので、
 * ここだけ切り抜いて出す (全体は押すと大きく出る) */
const SCREEN = { width: 1280, height: 720 }
const CROP = { x: 455, y: 45, width: 810, height: 455 }

/** 位置合わせ後の画面の、護石の欄と鑑定ボックスの切り抜き。読み取った値と見比べる用 */
export function AlignedScreen({ src }: { src: string }) {
  return (
    <Figure caption="読み取った画面" href={src} framed>
      <div className="relative w-full overflow-hidden" style={{ aspectRatio: `${CROP.width} / ${CROP.height}` }}>
        <img
          src={src}
          alt="位置合わせした画面 (護石の欄と鑑定ボックス)"
          className="absolute max-w-none"
          style={{
            width: `${(SCREEN.width / CROP.width) * 100}%`,
            left: `${(-CROP.x / CROP.width) * 100}%`,
            top: `${(-CROP.y / CROP.height) * 100}%`,
          }}
        />
      </div>
    </Figure>
  )
}

/** 写真を読む前 (と読めなかったとき) に、読み取った画面の場所に置く空の枠 (同じ大きさで、カードの形を変えないため) */
export function ScreenPlaceholder({ failed = false, className }: { failed?: boolean; className?: string }) {
  return (
    <figure className={cn('space-y-2 rounded-xl border bg-muted/50 p-3', className)}>
      <figcaption className="text-sm text-muted-foreground">読み取った画面</figcaption>
      <div
        className="flex flex-col items-center justify-center gap-2 rounded-lg border-2 border-dashed text-center text-sm text-muted-foreground"
        style={{ aspectRatio: `${CROP.width} / ${CROP.height}` }}
      >
        <ImageIcon className="size-6" />
        {failed ? '画面を読み取れませんでした' : '写真を読むと、ここに読み取った画面が出ます'}
      </div>
    </figure>
  )
}

/** framed: カードの中に置くとき、下地と枠で囲んで 1 つの区画に見せる */
function Figure({
  caption,
  href,
  framed = false,
  className,
  children,
}: {
  caption: string
  /** 押したときに大きく出す画像 */
  href: string
  framed?: boolean
  className?: string
  children: ReactNode
}) {
  return (
    <figure className={cn('space-y-1', framed && 'space-y-2 rounded-xl border bg-muted/50 p-3', className)}>
      <figcaption className="text-sm text-muted-foreground">{caption}</figcaption>
      {/* 押すとページの上に大きく出す (細部を拡大して確かめる用)。外を押すか Esc で閉じる */}
      <Dialog>
        <DialogTrigger
          render={
            <button
              type="button"
              aria-label={`${caption}を拡大`}
              className="block w-full cursor-zoom-in overflow-hidden rounded-lg border bg-black transition-opacity hover:opacity-90"
            />
          }
        >
          {children}
        </DialogTrigger>
        <DialogPortal>
          <DialogOverlay className="bg-black/80" />
          <DialogPrimitive.Popup className="fixed top-1/2 left-1/2 z-50 -translate-x-1/2 -translate-y-1/2 outline-none data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95">
            <DialogTitle className="sr-only">{caption}</DialogTitle>
            <img src={href} alt={caption} className="max-h-[92vh] max-w-[96vw] rounded-lg object-contain" />
            <DialogClose
              render={<Button variant="secondary" size="icon-sm" className="absolute top-2 right-2" aria-label="閉じる" />}
            >
              <XIcon />
            </DialogClose>
          </DialogPrimitive.Popup>
        </DialogPortal>
      </Dialog>
    </figure>
  )
}
