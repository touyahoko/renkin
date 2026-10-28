// 読み取りとフレーム探索を、画面を止めずに行う Worker。一度に 1 つだけ動かす (ページが裏に回ってしばらくすると、ページ側が捨てる)。
// 読み取りの準備 (埋め込んだ型紙の特徴点と見本の下ごしらえ、new Reader()) は最初に写真が来たときに 1 回だけ行い、あとは使い回す
import init, {
  Reader,
  searchFrames,
  skills,
  type Charm,
  type FrameCandidate,
  type ReadResult,
  type SkillInfo,
} from '../../renkin-reader/pkg/renkin_reader.js'

export type WorkerRequest =
  | { id: number; type: 'read'; file: Blob }
  | { id: number; type: 'search'; charm: Charm; index: number | null; start: number; end: number }
  /** 探索の中止。区切りの合間に受け付ける */
  | { id: number; type: 'cancel'; target: number }

export type WorkerResponse =
  | { type: 'ready'; skills: SkillInfo[] }
  | { id: number; type: 'read'; result: ReadResult; aligned: Blob | null }
  | { id: number; type: 'search'; found: FrameCandidate[] }
  /** 探索の進み具合 (探し終えたフレームの数 / 全体) */
  | { id: number; type: 'progress'; done: number; total: number }
  | { id: number; type: 'cancelled' }
  /** fatal: wasm の中で落ちた (または wasm を読み込めなかった) ので、この Worker はもう使えない。ページ側で作り直す */
  | { id: number; type: 'error'; message: string; fatal: boolean }
  /** wasm を読み込めなかった (ページ側で作り直す) */
  | { type: 'failed'; message: string }

/** 位置合わせ後の画面の大きさ (Reader.alignedImage() の画素の並び) */
const SCREEN = { width: 1280, height: 720 }

const post = (message: WorkerResponse) => self.postMessage(message)

const ready = init().then(() => post({ type: 'ready', skills: skills() }))
ready.catch((e: unknown) => post({ type: 'failed', message: e instanceof Error ? e.message : String(e) }))

/** 写真を開けなかった (wasm の状態には関係ないので、Worker はそのまま使える) */
class DecodeError extends Error {}

let reader: Reader | null = null

/** 探索は範囲をこの数ずつに分けて探し、区切りごとに進み具合を送る。フレームごとに独立に判定しているので、
 * 分けて探してつないでも、一度に探すのと同じ結果になる */
const CHUNK = 5_000_000
/** 中止を頼まれた探索 */
const cancelled = new Set<number>()

/** 写真を開いて RGBA の画素にする (写真の向き (EXIF) を反映する) */
async function decode(file: Blob): Promise<ImageData> {
  let bitmap: ImageBitmap
  try {
    bitmap = await createImageBitmap(file, { imageOrientation: 'from-image' })
  } catch {
    throw new DecodeError('この画像は開けません。HEIC は対応していないブラウザがあるので、JPEG で保存し直してください')
  }
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height)
  const context = canvas.getContext('2d')
  if (!context) throw new DecodeError('画像を処理できません')
  context.drawImage(bitmap, 0, 0)
  bitmap.close()
  return context.getImageData(0, 0, canvas.width, canvas.height)
}

/** 位置合わせ後の画面 (RGBA) を PNG にする */
async function toPng(rgba: Uint8Array): Promise<Blob> {
  const canvas = new OffscreenCanvas(SCREEN.width, SCREEN.height)
  const context = canvas.getContext('2d')
  if (!context) throw new Error('画像を処理できません')
  context.putImageData(new ImageData(new Uint8ClampedArray(rgba), SCREEN.width, SCREEN.height), 0, 0)
  return canvas.convertToBlob({ type: 'image/png' })
}

async function handle(request: WorkerRequest): Promise<void> {
  await ready
  switch (request.type) {
    case 'read': {
      const image = await decode(request.file)
      reader ??= new Reader()
      const result = reader.read(new Uint8Array(image.data.buffer), image.width, image.height)
      const rgba = reader.alignedImage()
      post({ id: request.id, type: 'read', result, aligned: rgba ? await toPng(rgba) : null })
      return
    }
    case 'search': {
      const { id, charm, index, start, end } = request
      const found: FrameCandidate[] = []
      for (let a = start; a < end; a = Math.min(a + CHUNK, end)) {
        found.push(...searchFrames(charm, index, a, Math.min(a + CHUNK, end)))
        post({ id, type: 'progress', done: Math.min(a + CHUNK, end) - start, total: end - start })
        // 区切りの合間に、中止のメッセージを受け取れるようにする
        await new Promise((r) => setTimeout(r))
        if (cancelled.delete(id)) {
          post({ id, type: 'cancelled' })
          return
        }
      }
      post({ id, type: 'search', found })
      return
    }
  }
}

self.onmessage = (event: MessageEvent<WorkerRequest>) => {
  const request = event.data
  // 中止は、探索の区切りを待たずにすぐ受け付ける (ready を待つ handle に回さない)
  if (request.type === 'cancel') {
    cancelled.add(request.target)
    return
  }
  handle(request).catch((e: unknown) => {
    // 写真を開けなかっただけなら続けて使える。それ以外 (wasm の中で落ちたなど) は wasm の状態が信用できないので、
    // ページ側にこの Worker を捨ててもらう
    const fatal = !(e instanceof DecodeError)
    if (fatal) console.error(e)
    post({ id: request.id, type: 'error', message: e instanceof Error ? e.message : String(e), fatal })
  })
}
