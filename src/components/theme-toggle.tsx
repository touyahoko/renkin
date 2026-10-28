import { MonitorIcon, MoonIcon, SunIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { useTheme, type Theme } from '@/lib/theme'

const ORDER: Theme[] = ['system', 'light', 'dark']

const LABEL: Record<Theme, string> = {
  system: '端末の設定に合わせる',
  light: 'ライト',
  dark: 'ダーク',
}

const ICON = {
  system: MonitorIcon,
  light: SunIcon,
  dark: MoonIcon,
}

/** 押すたびに 端末の設定 → ライト → ダーク と切り替わる */
export function ThemeToggle() {
  const { theme, setTheme } = useTheme()
  const Icon = ICON[theme]
  const next = ORDER[(ORDER.indexOf(theme) + 1) % ORDER.length]
  return (
    <Button
      variant="ghost"
      size="sm"
      aria-label={`表示テーマ: ${LABEL[theme]}。押すと${LABEL[next]}になります`}
      title={`表示テーマ: ${LABEL[theme]}`}
      onClick={() => setTheme(next)}
    >
      <Icon />
    </Button>
  )
}
