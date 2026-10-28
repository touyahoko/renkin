// スキルの一覧 (wasm の skills() から受け取る SkillInfo) を、画面の選択肢にする道具
import type { PointRange, SkillInfo } from './types'

/** 第 1 スキルの欄か、第 2 スキルの欄か */
export type Slot = 'skill1' | 'skill2'

export function skillName(skills: SkillInfo[], id: number | null): string {
  if (id === null) return 'なし'
  return skills.find((s) => s.id === id)?.name ?? `#${id}`
}

/** その欄に出せるスキル。読み取った値がその欄に出ないスキルでも、表示が消えないように足しておく */
export function skillsFor(skills: SkillInfo[], slot: Slot, current: number | null): SkillInfo[] {
  return skills.filter((s) => s[slot] != null || s.id === current)
}

/** ポイントの選択肢: 錬金で出る範囲。読み取った値が範囲の外なら、それも足しておく */
export function pointsFor(range: PointRange | null | undefined, current: number | null): number[] {
  const out = range ? Array.from({ length: range.max - range.min + 1 }, (_, i) => range.min + i) : []
  if (current !== null && !out.includes(current)) out.push(current)
  return out.sort((a, b) => a - b)
}

/** そのスキルとポイントが、その欄でマカフシギ錬金に出るか (どちらか未選択なら true) */
export function possible(skills: SkillInfo[], slot: Slot, id: number | null, pts: number | null): boolean {
  if (id === null || pts === null) return true
  const range = skills.find((s) => s.id === id)?.[slot]
  return range != null && range.min <= pts && pts <= range.max
}
