import { useEffect, useMemo, useRef, useState } from 'react'
import { AlertCircleIcon, CameraIcon, ImageIcon, LoaderCircleIcon, SearchIcon, ShieldCheckIcon } from 'lucide-react'

import { CharmForm } from '@/components/charm-form'
import { FrameResults } from '@/components/frame-results'
import { PhotoDropzone } from '@/components/photo-dropzone'
import { AlignedScreen, ScreenPlaceholder } from '@/components/photo-review'
import { SearchRangeFields } from '@/components/search-range-fields'
import { ThemeToggle } from '@/components/theme-toggle'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { isComplete, type Draft, type FieldName, type Unsure } from '@/lib/draft'
import { detectCapabilities } from '@/lib/capabilities'
import { useWindowFileDrop } from '@/lib/drop'
import { parseFrame } from '@/lib/frame'
import { formatBytes, hasFinePointer } from '@/lib/pointer'
import { loadSkills, readPhoto, searchFrames } from '@/lib/reader'
import { possible } from '@/lib/skills'
import type { FrameCandidate, ReadResult, RejectReason, SkillInfo } from '@/lib/types'

const EMPTY: Draft = { skill1: null, pts1: null, skill2: null, pts2: null, slots: null, index: null }

/** 撮り直しの理由ごとの案内 */
const RETAKE: Record<RejectReason, { title: string; text: string }> = {
  not_aligned: { title: '鑑定画面を読み取れませんでした', text: '鑑定ボックスまで画面全体が写るように撮り直してください。' },
  too_small: { title: '画面が小さく写っています', text: 'もう少し近づいて撮り直してください。' },
  panel_not_found: { title: '護石の欄を読み取れませんでした', text: '反射やピンボケがないように撮り直してください。' },
}

/** 読み取り結果から、画面に出す値と自信のない欄 */
function fromRead(r: Extract<ReadResult, { kind: 'read' }>): { draft: Draft; unsure: Unsure } {
  const f = r.fields
  const draft: Draft = {
    skill1: f.skill1.value,
    pts1: f.pts1.value,
    skill2: f.skill2.value,
    pts2: f.pts2.value,
    slots: f.slots.value,
    // 5 以上は鑑定ボックスに何回分もの錬金の護石が並んでいるときで、1 回の錬金の中での位置はわからない
    index: f.index.value !== null && f.index.value <= 4 ? f.index.value : null,
  }
  const unsure: Unsure = {}
  for (const name of Object.keys(draft) as FieldName[]) {
    // ボックス内の位置の候補は 1〜4 番目と不明だけ (5 以上は選べない)
    if (!f[name].confident) unsure[name] = f[name].candidates.filter((c) => name !== 'index' || c.value === null || c.value <= 4)
  }
  return { draft, unsure }
}

/** reading: 読み取り中 / rejected: 撮り直し / edit: 護石の確認と探索 */
type Stage = 'none' | 'reading' | 'rejected' | 'edit'

export default function App() {
  const [photo, setPhoto] = useState<{ url: string; name: string; detail: string } | null>(null)
  const [stage, setStage] = useState<Stage>('none')
  const [reason, setReason] = useState<RejectReason | null>(null)
  /** 位置合わせした画面の画像。撮り直し・読み取り中は null */
  const [aligned, setAligned] = useState<string | null>(null)
  const [draft, setDraft] = useState<Draft>(EMPTY)
  const [unsure, setUnsure] = useState<Unsure>({})
  const [startText, setStartText] = useState('0')
  const [endText, setEndText] = useState('10000000')
  const [searching, setSearching] = useState(false)
  const [found, setFound] = useState<{ list: FrameCandidate[]; indexKnown: boolean } | null>(null)
  const finePointer = useMemo(() => hasFinePointer(), [])
  /** マカフシギ錬金で出るスキルとポイントの範囲 (wasm から。読み取りの準備は最初の読み取りで行う) */
  const [skills, setSkills] = useState<SkillInfo[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  /** ブラウザが必要な機能 (WebAssembly の SIMD など) を備えているか。足りなければ wasm を読み込まない */
  const capabilities = useMemo(() => detectCapabilities(), [])

  useEffect(() => {
    if (!capabilities.ready) return
    loadSkills().then(setSkills, (e: Error) => setError(e.message))
  }, [capabilities.ready])

  // 選び直したら、前の写真と画面の URL を捨てる
  useEffect(
    () => () => {
      if (photo?.url.startsWith('blob:')) URL.revokeObjectURL(photo.url)
    },
    [photo],
  )
  useEffect(
    () => () => {
      if (aligned?.startsWith('blob:')) URL.revokeObjectURL(aligned)
    },
    [aligned],
  )

  /** 読み取りの結果を画面に出す */
  const show = (r: ReadResult, screen: string | null) => {
    setAligned(screen)
    if (r.kind === 'rejected') {
      // 前の写真の値が残っていると今の写真の結果に見えるので、空にして手で入力できるようにする
      setDraft(EMPTY)
      setUnsure({})
      setReason(r.reason)
      setStage('rejected')
      return
    }
    const next = fromRead(r)
    setDraft(next.draft)
    setUnsure(next.unsure)
    setStage('edit')
  }

  /** 最後に頼んだ読み取りの番号。読み取り中に別の写真を選んだら、前の読み取りの結果は捨てる */
  const readId = useRef(0)

  // 写真を選んだら、そのまま読み取る
  const select = async (file: File) => {
    const id = ++readId.current
    setPhoto({ url: URL.createObjectURL(file), name: file.name, detail: formatBytes(file.size) })
    setStage('reading')
    setFound(null)
    setAligned(null)
    setError(null)
    try {
      const { result, aligned: screen } = await readPhoto(file)
      if (id !== readId.current) return
      show(result, screen && URL.createObjectURL(screen))
    } catch (e) {
      if (id !== readId.current) return
      setError((e as Error).message)
      setStage('none')
    }
  }

  const reset = () => {
    // 読み取り中ならその結果は捨てる
    readId.current++
    setPhoto(null)
    setAligned(null)
    setDraft(EMPTY)
    setUnsure({})
    setStage('none')
    setFound(null)
    setError(null)
  }

  const dragging = useWindowFileDrop((file) => void select(file), !capabilities.ready || stage === 'reading' || searching)

  const change = (field: FieldName, value: number | null) => {
    setDraft((d) => ({ ...d, [field]: value }))
    // 自分で選んだ欄は確認済みにする
    setUnsure((u) => {
      const { [field]: _, ...rest } = u
      return rest
    })
    setFound(null)
  }

  const start = parseFrame(startText)
  const end = parseFrame(endText)
  // マカ錬金で出ない値なら、探しても必ず 0 件なので探させない
  const impossible =
    skills !== null &&
    (!possible(skills, 'skill1', draft.skill1, draft.pts1) || !possible(skills, 'skill2', draft.skill2, draft.pts2))
  const canSearch =
    isComplete(draft) && !impossible && start !== null && end !== null && start < end && !searching && stage !== 'reading'
  const unconfirmed = Object.keys(unsure).length > 0

  // 検索が終わったら結果のカードまで移る
  const resultCard = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!found) return
    resultCard.current?.scrollIntoView({
      behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth',
      block: 'start',
    })
    resultCard.current?.focus({ preventScroll: true })
  }, [found])

  /** 探索の進み具合 (0〜1) と中止 */
  const [progress, setProgress] = useState(0)
  const abort = useRef<AbortController | null>(null)

  const search = async () => {
    if (!canSearch) return
    setSearching(true)
    setProgress(0)
    const controller = new AbortController()
    abort.current = controller
    const charm = {
      skill1: draft.skill1!,
      pts1: draft.pts1!,
      skill2: draft.skill2,
      pts2: draft.skill2 === null ? null : draft.pts2,
      slots: draft.slots!,
    }
    try {
      const list = await searchFrames(charm, draft.index, start!, end!, {
        onProgress: (done, total) => setProgress(done / total),
        signal: controller.signal,
      })
      setFound({ list, indexKnown: draft.index !== null })
    } catch (e) {
      // 中止したときは何も出さない
      if ((e as Error).name !== 'AbortError') setError((e as Error).message)
    } finally {
      setSearching(false)
      abort.current = null
    }
  }

  return (
    <main className="mx-auto flex min-h-dvh w-full max-w-5xl flex-col gap-6 px-4 py-8">
      <header className="flex items-start justify-between gap-3">
        <div className="space-y-1">
          {/* ファビコンと同じアイコンを添えて、兄弟ツール (mhxx-combo-scan) と見分けやすくする */}
          <h1 className="flex items-center gap-3">
            <img src={`${import.meta.env.BASE_URL}favicon.svg`} alt="" className="size-9 shrink-0 sm:size-10" />
            {/* 狭い幅で入りきらないときは、バージョンだけが次の行に回る */}
            <span>
              MHXX 鑑定スキャン{' '}
              <span className="text-base font-normal whitespace-nowrap text-muted-foreground tabular-nums sm:text-lg">
                v{__APP_VERSION__}
              </span>
            </span>
          </h1>
          <p className="text-sm text-muted-foreground sm:text-base">鑑定画面の写真から、錬金時のフレームを特定します</p>
        </div>
        <ThemeToggle />
      </header>

      <p className="flex items-center justify-center gap-2 rounded-lg border border-primary/30 bg-primary/5 px-4 py-2.5 text-center text-sm font-medium sm:text-base">
        <ShieldCheckIcon className="size-5 shrink-0 text-primary" />
        写真は端末の中だけで処理され、どこにも送信されません
      </p>

      {dragging && (
        // 飾りなので、ドラッグのイベントを拾わせない
        <div className="pointer-events-none fixed inset-0 z-50 bg-primary/10 backdrop-blur-[2px]">
          <div className="absolute inset-3 flex flex-col items-center justify-center gap-3 rounded-2xl border-2 border-dashed border-primary bg-background/50 text-center sm:inset-6">
            <ImageIcon className="size-10 text-primary" />
            <p className="font-medium">ここにドロップ</p>
          </div>
        </div>
      )}

      {!capabilities.ready && (
        <Alert variant="destructive">
          <AlertCircleIcon />
          <AlertTitle>このブラウザでは動きません</AlertTitle>
          <AlertDescription>
            <ul className="list-inside list-disc">
              {capabilities.missing.map((m) => (
                <li key={m}>{m}</li>
              ))}
            </ul>
          </AlertDescription>
        </Alert>
      )}

      <PhotoDropzone
        onSelect={(file) => void select(file)}
        onReset={reset}
        selected={photo ?? undefined}
        reading={stage === 'reading'}
        disabled={!capabilities.ready || searching}
        showDropHint={finePointer}
      />

      {error && (
        <Alert variant="destructive">
          <AlertCircleIcon />
          <AlertTitle>読み込めませんでした</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {stage === 'rejected' && reason && (
        <Alert variant="destructive">
          <CameraIcon />
          <AlertTitle>{RETAKE[reason].title}</AlertTitle>
          <AlertDescription>{RETAKE[reason].text}</AlertDescription>
        </Alert>
      )}

      {/* 写真がなくても、手で入力して探せるように最初から出しておく */}
      {skills && (
        <Card>
          <CardContent className="space-y-4">
            {/* 読み取った画面と護石の欄を並べて、見比べながら直せるようにする (狭い幅では縦に並ぶ) */}
            {/* 画像の列は少し狭めて、高さを護石の欄に近づける (右の下に余白が出ないように) */}
            <div className="grid gap-6 md:grid-cols-[5fr_6fr]">
              {/* 写真を読むまでは同じ大きさの空の枠 (スマホでは場所を取るので出さない) */}
              {aligned ? <AlignedScreen src={aligned} /> : <ScreenPlaceholder failed={stage === 'rejected'} className="hidden md:block" />}
              {/* 横並びのときは、左の枠の内側の余白 (12px + 枠線) だけ下げて見出しの高さを揃え、左に薄い区切り線を引く */}
              <section className="space-y-3 md:border-l md:pt-[13px] md:pl-6">
                <h3>護石</h3>
                <CharmForm skills={skills} draft={draft} unsure={unsure} onChange={change} />
              </section>
            </div>
            <section className="space-y-2 border-t pt-4">
              <h3>探索範囲</h3>
              <SearchRangeFields
                start={startText}
                end={endText}
                onStartChange={setStartText}
                onEndChange={setEndText}
                disabled={searching}
              />
            </section>
            <div className="border-t pt-4">
              {searching ? (
                // 検索中は進み具合と中止
                <div className="space-y-2">
                  <div className="flex items-center justify-between gap-3">
                    <p className="flex items-center gap-2 text-sm text-muted-foreground">
                      <LoaderCircleIcon className="size-4 animate-spin" />
                      フレームを検索しています… {Math.floor(progress * 100)}%
                    </p>
                    <Button variant="outline" size="sm" onClick={() => abort.current?.abort()}>
                      中止
                    </Button>
                  </div>
                  <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                    <div
                      className="h-full rounded-full bg-primary transition-[width]"
                      style={{ width: `${Math.min(100, progress * 100)}%` }}
                    />
                  </div>
                </div>
              ) : (
                <div className="flex flex-wrap items-center gap-3">
                  <Button onClick={search} disabled={!canSearch}>
                    <SearchIcon />
                    フレーム検索
                  </Button>
                  {impossible ? (
                    <span className="text-sm text-destructive">マカ錬金では出ない値あり</span>
                  ) : (
                    unconfirmed && <span className="text-sm text-warning">読み取りが曖昧な欄あり</span>
                  )}
                </div>
              )}
            </div>
          </CardContent>
        </Card>
      )}

      <div ref={resultCard} tabIndex={-1} className="scroll-mt-4 outline-none">
        {found && (
          <FrameResults key={found.list.length} found={found.list} indexKnown={found.indexKnown} />
        )}
      </div>
    </main>
  )
}
