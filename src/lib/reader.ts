// ページ側から Worker (src/workers/reader.worker.ts) に読み取りとフレーム探索を頼む。
// Worker は一度に 1 つだけで、頼んだ順に 1 つずつ処理される。ページが裏に回ってしばらく何もしていなければ捨て
// (wasm のメモリは一度広がると縮まないので、Worker ごと返す)、次に頼んだときに作り直す
import type { Charm, FrameCandidate, ReadResult, SkillInfo } from '../../renkin-reader/pkg/renkin_reader.js'
import type { WorkerRequest, WorkerResponse } from '../workers/reader.worker'
import workerUrl from '../workers/reader.worker.ts?worker&url'

/** 頼む内容 (id は call が付ける)。ユニオンの各要素から id を外す */
type Request = WorkerRequest extends infer R ? (R extends unknown ? Omit<R, 'id'> : never) : never

type Pending = {
  resolve: (value: WorkerResponse) => void
  reject: (error: Error) => void
  /** 探索の進み具合 (探し終えたフレームの数, 全体) */
  onProgress?: (done: number, total: number) => void
}

let worker: Worker | null = null
let skillsReady: Promise<SkillInfo[]> | null = null
/** Worker の準備 (wasm の読み込み) が終わったか */
let ready = false
const pending = new Map<number, Pending>()
let nextId = 1

/** wasm の中で落ちたときなどに出す文 (元のエラーは Worker 側でコンソールに出す) */
const BROKEN = '処理中に問題が起きました。もう一度試すと、読み取りの準備からやり直します'

/** Worker を捨てる。待っている頼みごとは全部失敗にし、次の頼みごとで作り直す */
function discard(error: Error) {
  worker?.terminate()
  worker = null
  skillsReady = null
  ready = false
  for (const p of pending.values()) p.reject(error)
  pending.clear()
}

/** ページが裏に回ってから、この時間 (ミリ秒) 何もしていなければ Worker を捨てる。写真の選択画面を開いても
 * ページは裏に回るので、すぐには捨てない (捨てても、作り直しは wasm の読み込みだけで軽い) */
const RELEASE_AFTER_HIDDEN_MS = 10_000
let releaseTimer: ReturnType<typeof setTimeout> | undefined

/** 裏に回っていれば、しばらくしてから Worker を捨てる */
function scheduleRelease() {
  clearTimeout(releaseTimer)
  if (document.visibilityState === 'hidden') releaseTimer = setTimeout(release, RELEASE_AFTER_HIDDEN_MS)
}

/** 何もしていなければ Worker を捨てて、wasm のメモリをブラウザに返す。読み取りや探索の途中と、準備の途中は捨てない
 * (準備が終わるのを待っている頼みごとがあるため) */
function release() {
  if (!worker || !ready || pending.size > 0 || document.visibilityState !== 'hidden') return
  worker.terminate()
  worker = null
  skillsReady = null
  ready = false
}

document.addEventListener('visibilitychange', scheduleRelease)

/** Worker を作る。本物の Worker のスクリプトを、blob: の小さな Worker から読み込む。
 * 公開版の外部通信の禁止 (index.html の meta の CSP、vite.config.ts) は、URL から作った Worker には効かない
 * (Worker は自分のレスポンスヘッダーの CSP に従い、GitHub Pages はそれを付けない)。blob: から作った Worker は
 * ページの CSP を引き継ぐので、写真を扱う Worker の中でも外には通信できない */
function createWorker(): Worker {
  const source = `import ${JSON.stringify(new URL(workerUrl, window.location.href).href)}`
  const url = URL.createObjectURL(new Blob([source], { type: 'text/javascript' }))
  const created = new Worker(url, { type: 'module' })
  URL.revokeObjectURL(url)
  return created
}

function start(): Promise<SkillInfo[]> {
  if (skillsReady) return skillsReady
  skillsReady = new Promise((resolveSkills, rejectSkills) => {
    worker = createWorker()
    worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
      const message = event.data
      if (message.type === 'ready') {
        ready = true
        resolveSkills(message.skills)
        scheduleRelease()
        return
      }
      if (message.type === 'failed') {
        const error = new Error(`読み取りの準備に失敗しました (${message.message})`)
        rejectSkills(error)
        discard(error)
        return
      }
      const p = pending.get(message.id)
      if (!p) return
      if (message.type === 'progress') {
        p.onProgress?.(message.done, message.total)
        return
      }
      pending.delete(message.id)
      // 裏に回っている間に終わったら、そこから捨てるまでを数え直す
      if (pending.size === 0) scheduleRelease()
      if (message.type === 'error' && message.fatal) discard(new Error(BROKEN))
      if (message.type === 'error') p.reject(new Error(message.fatal ? BROKEN : message.message))
      else if (message.type === 'cancelled') p.reject(new DOMException('中止しました', 'AbortError'))
      else p.resolve(message)
    }
    worker.onerror = (event) => {
      const error = new Error(event.message || '読み取りの準備に失敗しました')
      rejectSkills(error)
      discard(error)
    }
  })
  return skillsReady
}

function call(
  request: Request,
  options: { onProgress?: Pending['onProgress']; signal?: AbortSignal } = {},
): Promise<WorkerResponse> {
  return start().then(
    () =>
      new Promise((resolve, reject) => {
        const id = nextId++
        pending.set(id, { resolve, reject, onProgress: options.onProgress })
        // 中止は Worker に頼み、区切りの合間に止まったら cancelled が返る
        options.signal?.addEventListener('abort', () => worker!.postMessage({ id: nextId++, type: 'cancel', target: id }), {
          once: true,
        })
        worker!.postMessage({ ...request, id } as WorkerRequest)
      }),
  )
}

/** wasm を読み込み、マカフシギ錬金で出るスキルとポイントの範囲を返す (読み取りの準備は最初の読み取りで行う) */
export function loadSkills(): Promise<SkillInfo[]> {
  return start()
}

/** 写真を読む。位置合わせ後の画面は PNG の Blob (撮り直しなら null) */
export async function readPhoto(file: Blob): Promise<{ result: ReadResult; aligned: Blob | null }> {
  const r = await call({ type: 'read', file })
  if (r.type !== 'read') throw new Error('読み取りの応答が不正')
  return { result: r.result, aligned: r.aligned }
}

/** 錬金時のフレームを探す。onProgress に進み具合 (探し終えたフレームの数, 全体) が届く。signal で中止できる
 * (中止したら AbortError で失敗する) */
export async function searchFrames(
  charm: Charm,
  index: number | null,
  start: number,
  end: number,
  options: { onProgress?: (done: number, total: number) => void; signal?: AbortSignal } = {},
): Promise<FrameCandidate[]> {
  const r = await call({ type: 'search', charm, index, start, end }, options)
  if (r.type !== 'search') throw new Error('探索の応答が不正')
  return r.found
}
