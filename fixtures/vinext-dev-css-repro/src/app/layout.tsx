import { resetCss } from '@devup-ui/reset-css'
import Header from './Header'
resetCss()
export default function Layout({ children }: { children: React.ReactNode }) {
  return <html lang="en"><body><Header />{children}</body></html>
}
