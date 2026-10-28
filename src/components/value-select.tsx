import { cn } from '@/lib/utils'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

export interface Option<T> {
  value: T
  label: string
}

/** 決まった選択肢から 1 つ選ぶ (ポイント、スロット、ボックス内の位置)。attention なら要確認の色にする */
export function ValueSelect<T>({
  value,
  onChange,
  options,
  placeholder,
  disabled = false,
  attention = false,
  className,
  id,
}: {
  value: T | null
  onChange: (v: T) => void
  options: Option<T>[]
  placeholder?: string
  disabled?: boolean
  attention?: boolean
  className?: string
  id?: string
}) {
  return (
    <Select
      items={options}
      value={value}
      onValueChange={(v) => {
        if (v !== null) onChange(v as T)
      }}
      disabled={disabled}
    >
      <SelectTrigger id={id} className={cn(className, attention && 'border-warning ring-3 ring-warning/20')}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {options.map((o) => (
          <SelectItem key={String(o.value)} value={o.value}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
