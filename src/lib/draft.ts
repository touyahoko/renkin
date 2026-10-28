import type { Candidate } from './types'

/** 画面で確かめて直す護石の中身 (null はまだ決まっていない。skill2 の null は第 2 スキルなし) */
export interface Draft {
  skill1: number | null
  pts1: number | null
  skill2: number | null
  pts2: number | null
  slots: number | null
  /** 鑑定ボックス内の位置 (1〜4)。null は不明 */
  index: number | null
}

export type FieldName = keyof Draft

/** 読み取りで自信がなかった欄と、そのときの候補 (点数の高い順) */
export type Unsure = Partial<Record<FieldName, Candidate<number | null>[]>>

export function isComplete(d: Draft): boolean {
  return d.skill1 !== null && d.pts1 !== null && d.slots !== null && (d.skill2 === null || d.pts2 !== null)
}
