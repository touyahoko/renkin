import { CircleAlertIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { SkillCombobox } from '@/components/skill-combobox'
import { ValueSelect, type Option } from '@/components/value-select'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import type { Draft, FieldName, Unsure } from '@/lib/draft'
import { pointsFor, possible, skillName, skillsFor, type Slot } from '@/lib/skills'
import type { SkillInfo } from '@/lib/types'

const SLOTS: Option<number>[] = [0, 1, 2, 3].map((n) => ({ value: n, label: String(n) }))

// ボックス内の位置は 1〜4 番目だけ (1 回の錬金で出るのは最大 4 個。5 以上は探索で「不明」と同じ扱いなので選ばせない)。
// 0 を「不明」に使う (選択肢の値に null は使えないため)
const INDEXES: Option<number>[] = [
  { value: 0, label: '不明' },
  ...[1, 2, 3, 4].map((n) => ({ value: n, label: `${n} 番目` })),
]

const showPoints = (p: number | null) => (p === null ? '-' : p > 0 ? `+${p}` : String(p))

/** 読み取った護石を表示し、ユーザーが確かめて直す。スキルとポイントの選択肢は、マカフシギ錬金で出るものだけ */
export function CharmForm({
  skills,
  draft,
  unsure,
  onChange,
}: {
  /** マカフシギ錬金で出るスキルとポイントの範囲 (wasm の skills()) */
  skills: SkillInfo[]
  draft: Draft
  unsure: Unsure
  onChange: (field: FieldName, value: number | null) => void
}) {
  const info = (id: number | null) => skills.find((s) => s.id === id)
  const show: Record<FieldName, (v: number | null) => string> = {
    skill1: (v) => skillName(skills, v),
    skill2: (v) => skillName(skills, v),
    pts1: showPoints,
    pts2: showPoints,
    slots: (v) => String(v),
    index: (v) => (v === null ? '不明' : `${v} 番目`),
  }

  // 欄ごとの自信の表示。自信がなければ候補を出し、押すとその値にする。錬金で出ない値なら、そのことも出す
  const status = (fields: FieldName[], slot?: Slot) => {
    const field = fields.find((f) => unsure[f])
    const impossible =
      slot === 'skill1'
        ? !possible(skills, 'skill1', draft.skill1, draft.pts1)
        : slot === 'skill2'
          ? !possible(skills, 'skill2', draft.skill2, draft.pts2)
          : false
    if (!field && !impossible) return null
    return (
      <div className="space-y-1.5">
        {field && (
          <>
            <p className="flex items-center gap-1.5 text-sm font-medium text-warning">
              <CircleAlertIcon className="size-4.5 shrink-0" />
              読み取りが曖昧
            </p>
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5">
              <span className="text-sm text-muted-foreground">候補:</span>
              {unsure[field]!.map((c) => (
                <Button
                  key={String(c.value)}
                  variant="outline"
                  size="xs"
                  onClick={() => onChange(field, c.value)}
                  title={`一致度 ${c.score.toFixed(2)}`}
                >
                  {show[field](c.value)}
                </Button>
              ))}
            </div>
          </>
        )}
        {impossible && (
          <p className="flex items-center gap-1.5 text-sm font-medium text-destructive">
            <CircleAlertIcon className="size-4.5 shrink-0" />
            マカ錬金では出ない値
          </p>
        )}
      </div>
    )
  }

  // スキルを選び直したら、そのスキルで出ないポイントは消す
  const changeSkill = (slot: Slot, id: number | null) => {
    const pts = slot === 'skill1' ? 'pts1' : 'pts2'
    onChange(slot, id)
    const range = info(id)?.[slot]
    const p = draft[pts]
    if (id === null || (p !== null && !(range && range.min <= p && p <= range.max))) onChange(pts, null)
  }

  const points = (slot: Slot) =>
    pointsFor(info(draft[slot])?.[slot], draft[slot === 'skill1' ? 'pts1' : 'pts2']).map((p) => ({
      value: p,
      label: showPoints(p),
    }))

  return (
    <div className="flex flex-col gap-6">
      <Row label="第1スキル" htmlFor="skill1" status={status(['skill1', 'pts1'], 'skill1')}>
        <SkillCombobox
          id="skill1"
          skills={skillsFor(skills, 'skill1', draft.skill1)}
          value={draft.skill1}
          onChange={(v) => changeSkill('skill1', v)}
          placeholder="スキル"
          attention={!!unsure.skill1}
        />
        <ValueSelect
          value={draft.pts1}
          onChange={(v) => onChange('pts1', v)}
          options={points('skill1')}
          placeholder="-"
          className="w-24"
          disabled={draft.skill1 === null}
          attention={!!unsure.pts1}
        />
      </Row>
      <Row label="第2スキル" htmlFor="skill2" status={status(['skill2', 'pts2'], 'skill2')}>
        <SkillCombobox
          id="skill2"
          skills={skillsFor(skills, 'skill2', draft.skill2)}
          value={draft.skill2}
          onChange={(v) => changeSkill('skill2', v)}
          allowNone
          attention={!!unsure.skill2}
        />
        <ValueSelect
          value={draft.pts2}
          onChange={(v) => onChange('pts2', v)}
          options={points('skill2')}
          placeholder="-"
          className="w-24"
          disabled={draft.skill2 === null}
          attention={!!unsure.pts2}
        />
      </Row>
      <Row label="スロット" htmlFor="slots" status={status(['slots'])}>
        <ValueSelect
          id="slots"
          value={draft.slots}
          onChange={(v) => onChange('slots', v)}
          options={SLOTS}
          placeholder="-"
          className="w-36"
          attention={!!unsure.slots}
        />
      </Row>
      <Row label="ボックス内の位置" htmlFor="index" status={status(['index'])}>
        <ValueSelect
          id="index"
          value={draft.index ?? 0}
          onChange={(v) => onChange('index', v === 0 ? null : v)}
          options={INDEXES}
          className="w-36"
          attention={!!unsure.index}
        />
      </Row>
    </div>
  )
}

function Row({
  label,
  htmlFor,
  status,
  children,
}: {
  label: string
  htmlFor: string
  status: ReactNode
  children: ReactNode
}) {
  return (
    <div className="grid gap-2 sm:grid-cols-[8.5rem_1fr] sm:items-start sm:gap-x-4">
      <Label htmlFor={htmlFor} className="pt-2 text-sm font-normal text-muted-foreground">
        {label}
      </Label>
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-2 *:first:flex-1">{children}</div>
        {status}
      </div>
    </div>
  )
}
