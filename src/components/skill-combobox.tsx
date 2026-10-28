import {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
} from '@/components/ui/combobox'
import type { SkillInfo } from '@/lib/types'
import { cn } from '@/lib/utils'

/** 一覧の先頭に置く「なし」(第 2 スキルなし)。選ぶと値は null になる */
const NONE: SkillInfo = { id: -1, name: 'なし', skill1: undefined, skill2: undefined }

/** スキルを名前で絞り込んで選ぶ。allowNone なら一覧の先頭に「なし」を置く。attention なら要確認の色にする */
export function SkillCombobox({
  skills,
  value,
  onChange,
  allowNone = false,
  placeholder,
  attention = false,
  id,
}: {
  /** 選べるスキル */
  skills: SkillInfo[]
  value: number | null
  onChange: (id: number | null) => void
  allowNone?: boolean
  placeholder?: string
  attention?: boolean
  id?: string
}) {
  const items = allowNone ? [NONE, ...skills] : skills
  const selected = skills.find((s) => s.id === value) ?? (allowNone ? NONE : null)
  return (
    <Combobox
      items={items}
      value={selected}
      onValueChange={(s: SkillInfo | null) => onChange(s === null || s.id === NONE.id ? null : s.id)}
      itemToStringLabel={(s: SkillInfo) => s.name}
      isItemEqualToValue={(a: SkillInfo, b: SkillInfo) => a.id === b.id}
    >
      <ComboboxInput
        id={id}
        className={cn('w-full', attention && 'border-warning ring-3 ring-warning/20')}
        placeholder={placeholder}
      />
      <ComboboxContent>
        <ComboboxEmpty>見つかりません</ComboboxEmpty>
        <ComboboxList>
          {(s: SkillInfo) => (
            <ComboboxItem key={s.id} value={s}>
              {s.name}
            </ComboboxItem>
          )}
        </ComboboxList>
      </ComboboxContent>
    </Combobox>
  )
}
