import { useCallback, useEffect, useState } from 'react'

export type Theme = 'system' | 'light' | 'dark'

/** index.html の先読みスクリプトと同じキーを使う。GitHub Pages では同じユーザーのツールどうしが
 * 1 つのオリジン (localStorage) を共有するので、ツールの名前を付けて mhxx-combo-scan などとぶつからないようにする */
const STORAGE_KEY = 'mhxx-renkin-reader:theme'

const DARK_QUERY = '(prefers-color-scheme: dark)'

export function readTheme(): Theme {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored === 'light' || stored === 'dark' || stored === 'system') return stored
  } catch {
    // プライベートウィンドウなどでは読めないことがある
  }
  return 'system'
}

export function resolveTheme(theme: Theme): 'light' | 'dark' {
  if (theme !== 'system') return theme
  return window.matchMedia(DARK_QUERY).matches ? 'dark' : 'light'
}

export function applyTheme(theme: Theme) {
  const resolved = resolveTheme(theme)
  const root = document.documentElement
  root.classList.toggle('dark', resolved === 'dark')
  // スクロールバーやフォーム部品の見た目も合わせる
  root.style.colorScheme = resolved
}

export function useTheme() {
  const [theme, setThemeState] = useState<Theme>(readTheme)

  useEffect(() => {
    applyTheme(theme)
    if (theme !== 'system') return
    // システム設定に追従している間だけ、変化を監視する
    const query = window.matchMedia(DARK_QUERY)
    const onChange = () => applyTheme('system')
    query.addEventListener('change', onChange)
    return () => query.removeEventListener('change', onChange)
  }, [theme])

  const setTheme = useCallback((next: Theme) => {
    try {
      localStorage.setItem(STORAGE_KEY, next)
    } catch {
      // 保存できなくても表示は切り替わる
    }
    setThemeState(next)
  }, [])

  return { theme, setTheme }
}

