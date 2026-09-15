# Critical Rules - Detailed Reference

## 1. React Compiler

Next.js projects **must** enable React Compiler:

```ts
// next.config.ts
const nextConfig = {
  experimental: {
    reactCompiler: true,  // Required!
  },
}
```

## 2. SEO Metadata

Each page must export `metadata` with **required `openGraph`**:

```tsx
// app/(auth)/my-page/page.tsx
import { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'My Page | Service',
  description: 'Manage your profile and settings.',
  openGraph: {
    title: 'My Page | Service',
    description: 'Manage your profile and settings.',
    images: ['/og-image.png'],
  },
}

export default function MyPagePage() {
  return <MyPageContent />
}
```

## 3. Image Alt Required

All `Image` and `img` tags **must** have `alt` attribute:

```tsx
// Good
<Image src="/logo.png" alt="Service logo" width={100} height={50} />

// Bad - missing alt
<Image src="/logo.png" width={100} height={50} />
```

## 4. Interactive States & Transitions

Interactive elements **must** have hover, active states with cursor and transition:

```tsx
// Good - all interactive states
<Box
  as="button"
  bg="$primary"
  color="#FFF"
  cursor="pointer"
  transition="all 0.2s ease"
  _hover={{ bg: '$primaryBold', transform: 'translateY(-1px)' }}
  _active={{ bg: '$primaryExBold', transform: 'scale(0.98)' }}
  _disabled={{ bg: '$gray200', color: '$gray400', cursor: 'not-allowed' }}
/>

// Bad - missing interactive states
<Box as="button" bg="$primary" color="#FFF" />
```

**Required checklist:**

| Element | Required Props |
|---------|----------------|
| Clickable | `cursor="pointer"` |
| Disabled state | `cursor="not-allowed"`, `_disabled={{...}}` |
| Hover effect | `_hover={{...}}` |
| Click effect | `_active={{...}}` |
| Smooth transition | `transition="all 0.2s ease"` or similar |

**Transition recommended values:**

```tsx
// Standard buttons/cards
transition="all 0.2s ease"

// Fast response (small icons)
transition="all 0.15s ease"

// Smooth animation (modals, panels)
transition="all 0.3s ease"
```

## 5. Export Rules

| Rule | Description |
|------|-------------|
| **One component per file** | Each file exports exactly one component |
| `export default` only in `page.tsx` | All other files use named export |
| Page components need descriptive name + `Page` suffix | e.g., `LoginPage`, `MyPagePage` (not just `Page`) |
| No `'use client'` in page.tsx | Page must be Server Component |

```tsx
// Good - one component per file
// Button.tsx
export function Button({ children }: ButtonProps) {
  return <Box>{children}</Box>
}

// Bad - multiple components in one file
// Button.tsx
export function Button({ children }: ButtonProps) {
  return <Box>{children}</Box>
}

export function IconButton({ icon }: IconButtonProps) {  // Should be in separate file!
  return <Box>{icon}</Box>
}
```

```tsx
// Good
export default function LoginPage() {
  return <LoginContent />
}

// Bad - just "Page"
export default function Page() {
  return <Content />
}

// Bad - 'use client' in page
'use client'  // Forbidden!
export default function LoginPage() { ... }
```

## 6. Server Component First (CRITICAL)

> Maximize Server Components to minimize JavaScript bundle size.

**Core principles:**
- Place base UI structure in `page.tsx`, `layout.tsx` (Server Component)
- Write components without `'use client'` by default
- Split only client-required parts minimally
- Use `children` pattern to pass Server Components to Client Components

### Correct Structure Pattern

```tsx
// Good - base structure in page.tsx (Server Component)
// app/(auth)/dashboard/page.tsx
import { Header } from '@/components/layout/Header'
import { Sidebar } from '@/components/layout/Sidebar'
import { DashboardContent } from '@/components/pages/dashboard/DashboardContent'

export default function DashboardPage() {
  return (
    <Flex>
      <Sidebar />           {/* Server Component */}
      <VStack flex={1}>
        <Header />          {/* Server Component */}
        <DashboardContent /> {/* Server Component */}
      </VStack>
    </Flex>
  )
}

// Bad - separate as client component
'use client'  // Unnecessary!
export function DashboardLayout() {
  return (
    <Flex>
      <Sidebar />
      <VStack flex={1}>
        <Header />
        <DashboardContent />
      </VStack>
    </Flex>
  )
}
```

### Client Separation Pattern (only when necessary)

```tsx
// Good - only client parts separated
// page.tsx (Server Component)
export default function ProductPage() {
  return (
    <VStack>
      <ProductInfo />           {/* Server Component - static info */}
      <ProductImageGallery />   {/* Server Component - image list */}
      <AddToCartButton />       {/* Client Component - button only */}
      <ProductReviews />        {/* Server Component - review list */}
    </VStack>
  )
}

// AddToCartButton.tsx - only part needing click handler is Client
'use client'
export function AddToCartButton() {
  const [loading, setLoading] = useState(false)
  return <Button onClick={...}>Add to Cart</Button>
}
```

### Client Hook 분리 패턴 (CRITICAL)

`'use client'`가 필요한 hook을 사용하는 부분만 별도 Client Component로 분리합니다.

**분리가 필요한 hook들:**
- `useRouter`, `usePathname`, `useSearchParams` (Next.js)
- `useState`, `useEffect`, `useRef` (React)
- `useContext` (Context)
- Custom hooks that use above

```tsx
// ❌ Bad - hook 때문에 전체가 Client Component
'use client'
export function DetailPageContent({ id }: { id: string }) {
  const router = useRouter()

  return (
    <>
      <PageHeader onBack={() => router.back()} title={`Item #${id}`} />
      {/* 많은 정적 UI 코드... */}
      <VStack gap="30px">
        <Table>...</Table>  {/* 전부 Server Component여도 됨 */}
        <Table>...</Table>
      </VStack>
      <Flex>
        <Button onClick={() => router.back()}>Cancel</Button>  {/* 이것 때문에 전체가 Client */}
        <Button>Save</Button>
      </Flex>
    </>
  )
}

// ✅ Good - hook 사용 부분만 분리
// DetailPageContent.tsx (Server Component)
export function DetailPageContent({ id }: { id: string }) {
  return (
    <>
      <PageHeaderWithBack title={`Item #${id}`} />  {/* Client */}
      <VStack gap="30px">
        <Table>...</Table>  {/* Server Component 유지! */}
        <Table>...</Table>
      </VStack>
      <DetailPageFooter />  {/* Client */}
    </>
  )
}

// PageHeaderWithBack.tsx (Client Component - 최소 범위)
'use client'
import { useRouter } from 'next/navigation'
import { PageHeader } from '@/components/layout/PageHeader'

export function PageHeaderWithBack({ title }: { title: string }) {
  const router = useRouter()
  return <PageHeader onBack={() => router.back()} title={title} />
}

// DetailPageFooter.tsx (Client Component - 최소 범위)
'use client'
import { useRouter } from 'next/navigation'

export function DetailPageFooter() {
  const router = useRouter()
  return (
    <Flex>
      <Button onClick={() => router.back()}>Cancel</Button>
      <Button>Save</Button>
    </Flex>
  )
}
```

**핵심:** hook이 필요한 부분만 최소 범위로 Client Component 추출! 나머지 정적 UI는 Server Component로 유지해서 JS 번들 최소화!

### Children Pattern (Key!)

```tsx
// Good - pass Server Components via children
// page.tsx (Server Component)
export default function ItemListPage() {
  return (
    <InteractiveWrapper>
      {/* These components all remain Server Components */}
      <ItemList />
      <Pagination />
      <FilterPanel />
    </InteractiveWrapper>
  )
}

// InteractiveWrapper.tsx - only interaction logic is Client
'use client'
export function InteractiveWrapper({ children }: { children: React.ReactNode }) {
  const [filter, setFilter] = useState(...)
  
  return (
    <FilterContext.Provider value={{ filter, setFilter }}>
      {children}  {/* Server Components passed as-is - not included in JS bundle! */}
    </FilterContext.Provider>
  )
}

// Bad - making entire component Client
'use client'
export function ItemListPage() {
  // All child components become Client Components - JS bundle size increases!
  return (
    <VStack>
      <ItemList />
      <Pagination />
      <FilterPanel />
    </VStack>
  )
}
```

### 'use client' Decision Guide

| Feature | 'use client' needed? | Reason |
|---------|---------------------|--------|
| Static UI rendering | No | Rendered on Server |
| Data fetch (async/await) | No | Direct fetch in Server Component |
| `useState`, `useEffect` | **Yes** | React hooks |
| `onClick`, `onChange` etc. **defined internally** | **Yes** | Browser events |
| `onClick` etc. **received via props** | **No** | Parent is already client if passing function |
| `useRouter`, `usePathname` | **Yes** | Next.js client hooks |
| Context usage (`useContext`) | **Yes** | Client-side state |
| Conditional rendering (props-based) | No | Can process on Server |
| `framer-motion` animation | **Yes** | Uses browser API |

**Important: Props-based Function Calls**

Components receiving optional functions via props don't need `'use client'` - but **only if you pass props directly without wrapping in arrow function**.

**Good patterns (can stay Server Component):**

```tsx
interface CardProps {
  onClick?: () => void
}

// Direct prop pass - NO 'use client' needed
export function Card({ onClick }: CardProps) {
  return <Box onClick={onClick}>Content</Box>
}

// Conditional direct pass - also OK
export function Card({ onClick }: CardProps) {
  return <Box onClick={onClick ? onClick : undefined}>Content</Box>
}

// Conditional with wrapper - also OK (only creates function when prop exists)
export function Card({ onClick }: CardProps) {
  return <Box onClick={onClick ? () => onClick() : undefined}>Content</Box>
}
```

**Anti-patterns (forces 'use client'):**

```tsx
// BAD - arrow function wrapper always creates a function
export function Card({ onClick }: CardProps) {
  return <Box onClick={() => onClick?.()}>Content</Box>  // Forces 'use client'!
}

// BAD - same problem
export function Card({ onClick }: CardProps) {
  return <Box onClick={() => onClick ? onClick() : undefined}>Content</Box>  // Forces 'use client'!
}
```

**Why?** Wrapping in arrow function (`() => ...`) means you're **defining** a new function internally, which requires `'use client'`. Direct prop pass (`onClick={onClick}`) doesn't define anything - it just passes the reference.

```tsx
// Needs 'use client' - defines handler internally
'use client'
export function Card({ title }: CardProps) {
  const handleClick = () => {  // Defines handler internally
    console.log('clicked')
  }
  
  return (
    <Box onClick={handleClick}>
      <Text>{title}</Text>
    </Box>
  )
}
```

### RSC async Rules

**Server Components without await must be sync:**

```tsx
// Good - has await, use async
async function UserProfilePage() {
  const user = await fetchUser()  // Uses await
  return <ProfileContent user={user} />
}

// Good - no await, use sync
function SettingsPage() {
  return <SettingsContent />  // No await -> sync
}

// Bad - async without await
async function AboutPage() {
  return <AboutContent />  // No await but using async
}
```

## 7. Fragment Rules

| Children count | Rule |
|----------------|------|
| **1** | Must remove Fragment |
| **2+** | Fragment allowed |

```tsx
// Good - 1 child: no Fragment needed
export default function HomePage() {
  return (
    <VStack>
      <Header />
      <Content />
    </VStack>
  )
}

// Bad - 1 child with Fragment
export default function HomePage() {
  return (
    <>
      <VStack>
        <Header />
        <Content />
      </VStack>
    </>
  )
}
```

## 8. Type Check & Lint Must Pass

> **CRITICAL: Run these commands in order after completing work!**

```bash
# 1. Type check
bun tsc --noEmit

# 2. Lint auto-fix
bun run lint:fix

# 3. Final lint check (must be 0 errors)
bun run lint
```

**Full run (recommended):**

```bash
bun tsc --noEmit && bun run lint:fix && bun run lint
```

## 9. Package Manager

**Default is bun.** Use other manager if different lock file exists:

| Lock File | Package Manager |
|-----------|-----------------|
| `bun.lock` | bun |
| `pnpm-lock.yaml` | pnpm |
| `package-lock.json` | npm |
| `yarn.lock` | yarn |

**Dev server execution:**

```bash
# Good - move to project directory and run
cd apps/front
bun dev

# Bad - don't use -F option (can cause zombie processes)
bun -F front dev
```

## 10. Testing Required

**Tests must be written for non-async components.**

### Test file location

```
src/
├── components/
│   ├── Button.tsx
│   ├── __tests__/
│   │   └── Button.test.tsx    # ComponentName.test.tsx
```

### Test setup

```bash
bun add -d @devup-ui/bun-plugin bun-test-env-dom
```

```toml
# bunfig.toml
[test]
preload = ["@devup-ui/bun-plugin", "bun-test-env-dom"]
```

### Test example

```tsx
// __tests__/Button.test.tsx
import { describe, expect, it } from 'bun:test'
import { render, screen, fireEvent } from '@testing-library/react'

import { Button } from '../Button'

describe('Button', () => {
  it('renders children correctly', () => {
    render(<Button variant="primary">Click me</Button>)
    expect(screen.getByText('Click me')).toBeInTheDocument()
  })

  it('calls onClick when clicked', () => {
    const handleClick = vi.fn()
    render(<Button variant="primary" onClick={handleClick}>Click</Button>)
    fireEvent.click(screen.getByText('Click'))
    expect(handleClick).toHaveBeenCalledTimes(1)
  })
})
```

## 11. Component Documentation

**Write detailed JSDoc comments for each component:**

```tsx
/**
 * Common button component
 * 
 * @description Base button used throughout the project.
 * Supports primary, secondary, disabled variants.
 * 
 * @example
 * ```tsx
 * <Button variant="primary" onClick={handleClick}>
 *   Save
 * </Button>
 * ```
 * 
 * @param variant - Button style ('primary' | 'secondary' | 'disabled')
 * @param children - Button content
 * @param onClick - Click event handler
 * @param disabled - Disabled state
 */
export function Button({ variant, children, onClick, disabled }: ButtonProps) {
  // ...
}
```

## 12. Query Error & Loading States

Client components using react-query **must handle `isError`, `isPending` states**:

```tsx
'use client'
import { queryApi } from '@/api'

export function UserList() {
  const { data, isError, isPending, error } = queryApi.useQuery('get', '/users')

  // Must handle loading state
  if (isPending) {
    return <Loading message="Loading users..." />
  }

  // Must handle error state
  if (isError) {
    return <ErrorMessage message="Failed to load users." error={error} />
  }

  return (
    <VStack gap={3}>
      {data.map((user) => (
        <UserCard key={user.id} user={user} />
      ))}
    </VStack>
  )
}
```

## 13. Use @devup-ui/components

**Maximize use of @devup-ui/components package.**

Components like Checkbox, Select, Input, Button already exist in `@devup-ui/components`. **Use existing components instead of creating new ones.**

```tsx
// Good - use @devup-ui/components
import { Checkbox, Select, Input, Button } from '@devup-ui/components'

// Bad - recreating existing component
export function Checkbox({ checked, onChange, children }) {
  return (
    <label>
      <input type="checkbox" checked={checked} onChange={onChange} />
      {children}
    </label>
  )
}
```

## 14. Interface over Type

**Use `interface` instead of `type`:**

```tsx
// Good
interface ButtonProps {
  variant: 'primary' | 'secondary'
  children: React.ReactNode
}

// Bad
type ButtonProps = {
  variant: 'primary' | 'secondary'
  children: React.ReactNode
}
```

**Exceptions where `type` is allowed:**

| Situation | Example |
|-----------|---------|
| Union types | `type Status = 'idle' \| 'loading' \| 'error'` |
| Intersection types | `type Combined = A & B` |
| Mapped types | `type Readonly<T> = { readonly [K in keyof T]: T[K] }` |

## 15. Regular Functions over Arrow Functions

**Use regular function instead of arrow function:**

```tsx
// Good
export function Button({ children }: ButtonProps) {
  return <Box>{children}</Box>
}

function handleClick() {
  console.log('clicked')
}

// Bad
export const Button = ({ children }: ButtonProps) => {
  return <Box>{children}</Box>
}

const handleClick = () => {
  console.log('clicked')
}
```

**Exceptions where arrow function is allowed:**

| Situation | Example |
|-----------|---------|
| Inline callback | `onClick={() => setState(true)}` |
| Array methods | `items.map((item) => <Item key={item.id} />)` |
| Immediate return | `const double = (n: number) => n * 2` |

## 16. Use Next.js Link for Navigation

> **Link works in Server Components, but useRouter needs 'use client'.**
> **Using Link enables navigation without 'use client', reducing JS bundle size.**

```tsx
// Good - Link in Server Component (no 'use client' needed!)
import Link from 'next/link'

export function Navbar() {  // No 'use client'!
  return (
    <nav>
      <Link href="/">Home</Link>
      <Link href="/products">Products</Link>
    </nav>
  )
}

// Bad - useRouter requires 'use client'
'use client'  // Required because of useRouter!
import { useRouter } from 'next/navigation'

export function Navbar() {
  const router = useRouter()
  return (
    <nav>
      <button onClick={() => router.push('/')}>Home</button>
    </nav>
  )
}
```

**useRouter allowed cases (programmatic navigation only):**

```tsx
// Good - conditional navigation after API call (useRouter allowed)
'use client'
export function CreatePostForm() {
  const router = useRouter()
  
  async function handleSubmit() {
    const result = await api.createPost(data)
    if (result.success) {
      router.push(`/posts/${result.id}`)  // Conditional - OK
    }
  }
  
  return <form onSubmit={handleSubmit}>...</form>
}
```

## 17. No Unnecessary Parameters & Variables

**Must remove unused parameters and variables:**

```tsx
// Good - declare only what's needed
function UserCard({ name, email }: UserCardProps) {
  return (
    <Box>
      <Text>{name}</Text>
      <Text>{email}</Text>
    </Box>
  )
}

// Bad - unused parameters
function UserCard({ name, email, id, createdAt }: UserCardProps) {
  // id, createdAt not used
  return (
    <Box>
      <Text>{name}</Text>
      <Text>{email}</Text>
    </Box>
  )
}
```

## 18. SVG Icon Files

**Single-color SVG icons must be stored as `.svg` files in `public/icons/`, NOT as React components.**

### Why?

- Reduces JS bundle size (SVG files are not bundled)
- Better caching (static assets)
- Simpler maintenance

### Decision Guide

| SVG Type | Location | Format |
|----------|----------|--------|
| Single-color icon | `public/icons/` | `.svg` file |
| Multi-color with dynamic props | `components/` | React component |
| Animated/interactive SVG | `components/` | React component |

### Single-color SVG (use file)

```svg
<!-- public/icons/arrow-right.svg -->
<svg viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg">
  <path d="M12 4l-1.41 1.41L16.17 11H4v2h12.17l-5.58 5.59L12 20l8-8z"/>
</svg>
```

**Usage:**

```tsx
// With Next.js Image
import Image from 'next/image'

<Image src="/icons/arrow-right.svg" alt="arrow right" width={24} height={24} />

// With CSS for color control
<Box
  as="img"
  src="/icons/arrow-right.svg"
  alt="arrow"
  w={24}
  h={24}
  filter="invert(1)"  // For white on dark background
/>
```

### Multi-color/Dynamic SVG (use component)

```tsx
// components/icons/StatusIcon.tsx - OK as component (multi-color, dynamic)
interface StatusIconProps {
  status: 'success' | 'error' | 'warning'
}

export function StatusIcon({ status }: StatusIconProps) {
  const colors = {
    success: { bg: '#4CAF50', icon: '#FFF' },
    error: { bg: '#F44336', icon: '#FFF' },
    warning: { bg: '#FF9800', icon: '#000' },
  }
  
  return (
    <svg viewBox="0 0 24 24">
      <circle fill={colors[status].bg} cx="12" cy="12" r="10" />
      <path fill={colors[status].icon} d="..." />
    </svg>
  )
}
```

### Anti-pattern

```tsx
// Bad - single-color SVG as React component
// components/icons/ArrowRightIcon.tsx
export function ArrowRightIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor">
      <path d="M12 4l-1.41 1.41L16.17 11H4v2h12.17l-5.58 5.59L12 20l8-8z"/>
    </svg>
  )
}
// This adds unnecessary JS to bundle!
// Should be: public/icons/arrow-right.svg
```

## 19. HTML suppressHydrationWarning (Theme)

> **html 태그에 `suppressHydrationWarning` 필수!**

다크모드/라이트모드 테마 전환 시 서버-클라이언트 hydration 불일치 warning을 방지합니다.

```tsx
// src/app/layout.tsx
export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="ko" suppressHydrationWarning>  {/* REQUIRED! */}
      <body>
        <Provider>{children}</Provider>
      </body>
    </html>
  )
}
```

**왜 필요한가?**
- 테마(다크모드)는 클라이언트 localStorage/cookie에서 결정됨
- 서버 렌더링 시 테마를 알 수 없어 hydration mismatch 발생
- `suppressHydrationWarning`으로 해당 warning 억제

**Anti-pattern:**
```tsx
// Bad - suppressHydrationWarning 없음
<html lang="ko">  {/* Warning 발생! */}
```

## 20. Footer Background Strategy

> **Footer가 있다면 body 배경색 = Footer 배경색으로 설정!**

컨텐츠 높이가 짧을 때 Footer만 다른 배경색이면 어색하게 보입니다.

### 문제 상황

```
┌─────────────────────┐
│  Header (흰색)       │
├─────────────────────┤
│                     │
│  Content (흰색)      │  ← 컨텐츠가 짧으면
│                     │
├─────────────────────┤
│  Footer (회색)       │  ← Footer만 튀어 보임
├─────────────────────┤
│  빈 공간 (???)       │  ← body 색상이 뭐지?
└─────────────────────┘
```

### 해결: Body 배경 = Footer 배경

```tsx
// src/app/layout.tsx
export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="ko" suppressHydrationWarning>
      <body>
        {/* body 자체는 Footer와 같은 배경색 */}
        <Box bg="$footerBackground" minH="100vh">
          {/* 본문 영역에 별도 배경색 */}
          <Box bg="$background" minH="100vh">
            <Header />
            <Box as="main" flex={1}>
              {children}
            </Box>
          </Box>
          <Footer />  {/* Footer는 $footerBackground 사용 */}
        </Box>
      </body>
    </html>
  )
}
```

### 또는 globalCss로 body 배경 설정

```tsx
// src/app/layout.tsx
import { globalCss } from '@devup-ui/react'

const globalStyles = globalCss({
  body: {
    bg: '$footerBackground',  // Footer와 동일한 배경색
  },
})

export default function RootLayout({ children }: { children: React.ReactNode }) {
  globalStyles()
  
  return (
    <html lang="ko" suppressHydrationWarning>
      <body>
        <VStack minH="100vh">
          <Header />
          <Box as="main" flex={1} bg="$background">
            {children}
          </Box>
          <Footer />
        </VStack>
      </body>
    </html>
  )
}
```

### 결과

```
┌─────────────────────┐
│  Header             │
├─────────────────────┤
│  Content ($bg)      │  ← 본문은 별도 배경
├─────────────────────┤
│  Footer ($footerBg) │  ← body와 같은 색
├─────────────────────┤
│  (빈공간=$footerBg) │  ← 자연스럽게 연결됨
└─────────────────────┘
```

**핵심:**
- `body` 배경색 = `Footer` 배경색
- `main/content` 영역에만 별도 배경색 (`$background`)
- 컨텐츠가 짧아도 Footer 아래가 자연스러움

## 21. as const for Literal Types

> **별도 타입 정의 없이 object/array를 선언할 때는 `as const` 사용!**

`as const`를 사용하면 TypeScript가 더 구체적인 리터럴 타입을 추론합니다.

### Array

```tsx
// Bad - string[] 으로 추론됨
const sizes = ['sm', 'md', 'lg']
// type: string[]

// Good - readonly ['sm', 'md', 'lg'] 으로 추론됨
const sizes = ['sm', 'md', 'lg'] as const
// type: readonly ['sm', 'md', 'lg']

// 활용: 타입으로 사용 가능
type Size = typeof sizes[number]  // 'sm' | 'md' | 'lg'
```

### Object

```tsx
// Bad - { theme: string, size: string } 으로 추론됨
const config = {
  theme: 'dark',
  size: 'lg',
}
// type: { theme: string, size: string }

// Good - 리터럴 타입으로 추론됨
const config = {
  theme: 'dark',
  size: 'lg',
} as const
// type: { readonly theme: 'dark', readonly size: 'lg' }
```

### Style Variants 패턴

```tsx
// Good - as const로 variant 키를 타입으로 활용
const buttonVariants = {
  primary: css({ bg: '$primary', color: '#FFF' }),
  secondary: css({ bg: '$gray100', color: '$text' }),
  ghost: css({ bg: 'transparent', color: '$text' }),
} as const

type ButtonVariant = keyof typeof buttonVariants
// 'primary' | 'secondary' | 'ghost'

interface ButtonProps {
  variant: ButtonVariant  // 타입 안전!
}
```

### 언제 사용하는가?

| 상황 | as const 사용 |
|------|--------------|
| 별도 타입 정의 없이 object/array 선언 | **Yes** |
| 상수 값으로 사용될 리터럴 | **Yes** |
| variant, status 등 고정된 옵션 | **Yes** |
| 런타임에 값이 변경될 수 있는 경우 | No |
| 이미 명시적 타입이 있는 경우 | No |

### 실전 예시

```tsx
// Status tags
const statusConfig = {
  success: { bg: '$greenTagBg', color: '$greenTag', label: '성공' },
  error: { bg: '$redTagBg', color: '$redTag', label: '실패' },
  pending: { bg: '$orangeTagBg', color: '$orangeTag', label: '대기' },
} as const

type Status = keyof typeof statusConfig  // 'success' | 'error' | 'pending'

// Navigation items
const navItems = [
  { href: '/', label: '홈' },
  { href: '/products', label: '상품' },
  { href: '/about', label: '소개' },
] as const

// API endpoints
const endpoints = {
  users: '/api/users',
  products: '/api/products',
  orders: '/api/orders',
} as const
```

## 22. Inline Variant Style Pattern

> **size/variant에 따른 스타일은 별도 config 객체가 아닌 인라인 객체 인덱싱 사용!**

스타일 값을 컴포넌트 외부에 별도 객체로 분리하지 않고, JSX 내에서 인라인으로 정의합니다.

### Anti-Pattern (별도 config 객체)

```tsx
// Bad - 별도 config 객체 정의
const sizeConfig = {
  sm: {
    trackW: '36px',
    trackH: '20px',
    thumbSize: '16px',
    thumbOffset: '2px',
    thumbMove: '16px',
  },
  md: {
    trackW: '44px',
    trackH: '24px',
    thumbSize: '20px',
    thumbOffset: '2px',
    thumbMove: '20px',
  },
}

export function Toggle({ size = 'md' }: ToggleProps) {
  const config = sizeConfig[size]  // 불필요한 변수
  
  return (
    <Flex w={config.trackW} h={config.trackH} p={config.thumbOffset}>
      <Box boxSize={config.thumbSize} />
    </Flex>
  )
}
```

### Good Pattern (인라인 객체 인덱싱)

```tsx
// Good - 인라인 객체 인덱싱
export function Toggle({ size = 'md' }: ToggleProps) {
  return (
    <Flex
      w={{ sm: '36px', md: '44px' }[size]}
      h={{ sm: '20px', md: '24px' }[size]}
      p={{ sm: '2px', md: '2px' }[size]}
    >
      <Box
        boxSize={{ sm: '16px', md: '20px' }[size]}
        transform={{
          sm: checked ? 'translateX(16px)' : 'translateX(0)',
          md: checked ? 'translateX(20px)' : 'translateX(0)',
        }[size]}
      />
    </Flex>
  )
}
```

### 완전한 예시

```tsx
'use client'

import { Box, Flex, Text } from '@devup-ui/react'

interface ToggleProps {
  checked?: boolean
  onChange?: (checked: boolean) => void
  disabled?: boolean
  size?: 'sm' | 'md'
  label?: string
}

export function Toggle({
  checked = false,
  onChange,
  disabled = false,
  size = 'md',
  label,
}: ToggleProps) {
  return (
    <Flex
      alignItems="center"
      cursor={disabled ? 'not-allowed' : 'pointer'}
      gap={2}
      onClick={() => !disabled && onChange?.(!checked)}
    >
      <Flex
        alignItems="center"
        bg={checked ? '$primary' : '$gray200'}
        borderRadius="1000px"
        h={{ sm: '20px', md: '24px' }[size]}
        p="2px"
        transition="all 0.2s ease"
        w={{ sm: '36px', md: '44px' }[size]}
        _hover={disabled ? undefined : { bg: checked ? '$primaryDark' : '$gray300' }}
      >
        <Box
          bg="$white"
          borderRadius="50%"
          boxShadow="0 0 6px 0 #0000001A"
          boxSize={{ sm: '16px', md: '20px' }[size]}
          transform={{
            sm: checked ? 'translateX(16px)' : 'translateX(0)',
            md: checked ? 'translateX(20px)' : 'translateX(0)',
          }[size]}
          transition="transform 0.2s ease"
        />
      </Flex>
      {label && (
        <Text color={disabled ? '$gray400' : '$text'} typography="$body">
          {label}
        </Text>
      )}
    </Flex>
  )
}
```

### 장점

| 장점 | 설명 |
|------|------|
| **코로케이션** | 스타일이 사용되는 위치에 정의되어 가독성 향상 |
| **불필요한 변수 제거** | `const config = sizeConfig[size]` 같은 중간 변수 불필요 |
| **수정 용이** | 특정 속성 수정 시 해당 줄만 보면 됨 |
| **타입 안전** | TypeScript가 인덱스 타입 체크 |

### typography prop에서 as const 필수

`typography` prop은 특정 리터럴 타입만 허용하므로 `as const` 필수:

```tsx
// ✅ Best - typography에는 (객체 as const)[key]
<Text
  typography={
    (
      {
        lg: 'buttonLg',
        md: 'button',
        sm: 'buttonSm',
        tag: 'tag',
      } as const
    )[size]
  }
/>
// 결과 타입: 'buttonLg' | 'button' | 'buttonSm' | 'tag' ✓

// ⚠️ Fallback - 결과에 타입 단언
<Text
  typography={
    { lg: 'buttonLg', md: 'button', sm: 'buttonSm', tag: 'tag' }[size] as
      | 'buttonLg'
      | 'button'
      | 'buttonSm'
      | 'tag'
  }
/>
```

**일반 스타일 prop은 as const 불필요:**

```tsx
// ✅ OK - w, h, fontSize 등은 string 허용
<Box
  w={{ md: '44px', sm: '36px' }[size]}
  h={{ md: '24px', sm: '20px' }[size]}
  fontSize={{ md: '16px', sm: '14px' }[size]}
/>
// as const 없어도 됨 - string 타입으로 충분
```

**as const가 필요한 prop:**

| Prop | as const 필요 | 이유 |
|------|--------------|------|
| `typography` | **Yes** | 특정 리터럴 타입만 허용 |
| `w`, `h`, `p`, `m` 등 | No | string 허용 |
| `fontSize`, `color` 등 | No | string 허용 |
| `bg`, `border` 등 | No | string 허용 |

### 예외: css() 함수 사용 시

`css()` 함수로 className을 생성하는 경우는 외부 객체 정의 허용:

```tsx
// OK - css()는 빌드 타임에 처리되므로 외부 정의 가능
const buttonVariants = {
  primary: css({ bg: '$primary', color: '#FFF' }),
  secondary: css({ bg: '$gray100', color: '$text' }),
} as const

export function Button({ variant = 'primary' }: ButtonProps) {
  return (
    <Box className={buttonVariants[variant]} styleOrder={1}>
      {children}
    </Box>
  )
}
```

## 23. No Barrel Files (index.tsx)

> **export만을 위한 `index.tsx` (barrel file)는 만들지 않는다!**

re-export만 하는 index.tsx 파일은 불필요하고 문제를 야기할 수 있습니다.

### Anti-Pattern (Barrel File)

```
components/
├── common/
│   ├── Button.tsx
│   ├── Input.tsx
│   ├── Modal.tsx
│   └── index.tsx      ← 불필요! 만들지 않음
```

```tsx
// components/common/index.tsx - Bad!
export { Button } from './Button'
export { Input } from './Input'
export { Modal } from './Modal'
```

```tsx
// 사용처 - barrel file 통해 import
import { Button, Input } from '@/components/common'  // Bad
```

### Good Pattern (Direct Import)

```
components/
├── common/
│   ├── Button.tsx
│   ├── Input.tsx
│   └── Modal.tsx
│   (index.tsx 없음!)
```

```tsx
// 사용처 - 직접 import
import { Button } from '@/components/common/Button'  // Good
import { Input } from '@/components/common/Input'    // Good
```

### 왜 Barrel File을 피하는가?

| 문제 | 설명 |
|------|------|
| **Tree-shaking 방해** | 번들러가 사용하지 않는 export를 제거하기 어려움 |
| **순환 참조 위험** | 여러 파일이 서로 의존할 때 circular dependency 발생 |
| **빌드 성능 저하** | 하나의 컴포넌트 변경 시 index.tsx도 재빌드 |
| **불필요한 파일** | 실제 로직 없이 파일만 늘어남 |
| **IDE 성능** | 큰 barrel file은 자동완성 성능 저하 |

### 예외: 패키지 진입점

npm 패키지의 진입점으로 사용되는 경우는 허용:

```tsx
// packages/ui/src/index.ts - OK (패키지 진입점)
export { Button } from './Button'
export { Input } from './Input'
```

**하지만 앱 내부 컴포넌트에서는 사용하지 않음!**

### 정리

| 상황 | index.tsx 생성 |
|------|---------------|
| 앱 내부 컴포넌트 폴더 | **No** |
| 앱 내부 utils, hooks 폴더 | **No** |
| npm 패키지 진입점 | OK |
| 라이브러리 public API | OK |

## 24. Avoid Closure in Props (Unnecessary 'use client')

> **클로저로 값을 캡처하는 대신, 자식에게 값을 전달하고 자식이 콜백을 처리하게 한다!**

부모에서 `onChange={() => onChange?.(value)}` 형태로 클로저를 만들면 불필요한 `'use client'`가 필요합니다.

### Anti-Pattern (불필요한 'use client')

```tsx
// RadioGroup.tsx
'use client'  // 불필요함!

export function RadioGroup({ options, value, onChange }: RadioGroupProps) {
  return (
    <Flex>
      {options.map((option) => (
        <Radio
          key={option.value}
          selected={value === option.value}
          label={option.label}
          // Bad - 클로저가 option.value를 캡처 → 'use client' 필요
          onChange={() => onChange?.(option.value)}
        />
      ))}
    </Flex>
  )
}
```

### Good Pattern (Server Component 유지)

```tsx
// RadioGroup.tsx
// 'use client' 없음! Server Component 유지

export function RadioGroup({ options, value, onChange }: RadioGroupProps) {
  return (
    <Flex>
      {options.map((option) => (
        <Radio
          key={option.value}
          selected={value === option.value}
          label={option.label}
          // Good - 값을 자식에게 전달
          value={option.value}
          onChange={onChange}
        />
      ))}
    </Flex>
  )
}

// Radio.tsx
'use client'  // 실제 이벤트 핸들링은 여기서

interface RadioProps {
  selected?: boolean
  label?: string
  value: string
  onChange?: (value: string) => void
}

export function Radio({ selected, label, value, onChange }: RadioProps) {
  return (
    <Flex
      // Good - 조건부로 함수 생성 (onChange 있을 때만)
      onClick={onChange ? () => onChange(value) : undefined}
      cursor="pointer"
    >
      {/* Radio UI */}
    </Flex>
  )
}
```

### 패턴 비교

| 패턴 | 부모 | 자식 | 결과 |
|------|------|------|------|
| **Bad** | `onChange={() => onChange?.(value)}` | `onClick={onChange}` | 부모가 'use client' 필요 |
| **Good** | `onChange={onChange}` + `value={value}` | `onClick={onChange ? () => onChange(value) : undefined}` | 부모는 Server Component 유지 |

### 핵심 원칙

```
항상 함수 생성 = 'use client' 필요

✗ onClick={() => handler?.(value)}              // 항상 함수 생성 (Bad)
✓ onClick={handler ? () => handler(value) : undefined}  // 조건부 생성 (Good)
```

### 실전 적용

```tsx
// SelectGroup.tsx - Server Component
export function SelectGroup({ items, selected, onSelect }: Props) {
  return (
    <VStack>
      {items.map((item) => (
        <SelectItem
          key={item.id}
          item={item}
          isSelected={selected === item.id}
          onSelect={onSelect}  // 함수 자체를 전달
        />
      ))}
    </VStack>
  )
}

// SelectItem.tsx - Client Component
'use client'

export function SelectItem({ item, isSelected, onSelect }: Props) {
  return (
    // Good - 조건부로 함수 생성
    <Box onClick={onSelect ? () => onSelect(item.id) : undefined}>
      {item.label}
    </Box>
  )
}
```

### 이점

| 이점 | 설명 |
|------|------|
| **JS 번들 감소** | 부모가 Server Component로 유지되어 번들에 포함 안됨 |
| **명확한 책임** | 이벤트 핸들링은 실제로 필요한 컴포넌트에서만 |
| **테스트 용이** | 부모는 순수 렌더링, 자식은 인터랙션 담당 |

### 주의: Optional Chaining vs Conditional

```tsx
// ❌ Bad - 항상 함수를 생성함
onClick={() => onChange?.(value)}

// ✅ Good - onChange가 있을 때만 함수 생성
onClick={onChange ? () => onChange(value) : undefined}
```

| 패턴 | 함수 생성 | 결과 |
|------|----------|------|
| `() => fn?.(v)` | **항상** | 'use client' 필요 |
| `fn ? () => fn(v) : undefined` | **조건부** | Server Component 가능 |

## 25. $color vs var(--color) Token Syntax

> **JSX prop에 직접 전달할 때만 `$color`, 외부 객체에서는 `var(--color)` 사용!**

devup-ui의 `$` 토큰 문법은 **빌드 타임에 JSX prop에서만 처리**됩니다. 외부 객체에 정의하면 변환되지 않습니다.

### Anti-Pattern (외부 객체에 $color)

```tsx
// Bad - 외부 객체에 $color 사용
const typeColors: Record<ToastType, string> = {
  success: '$success',  // 변환 안됨!
  error: '$error',
  warning: '$warning',
}

function ToastItem({ toast }: Props) {
  return (
    <Flex bg={typeColors[toast.type]}>  {/* $success 문자열 그대로 */}
      ...
    </Flex>
  )
}
```

### Good Pattern 1: 인라인 객체 인덱싱 + $color

```tsx
// Good - 인라인 객체에서 $color 사용 (JSX prop 내부이므로 변환됨)
function ToastItem({ toast }: Props) {
  return (
    <Flex
      bg={{
        success: '$success',
        error: '$error',
        warning: '$warning',
        info: '$info',
      }[toast.type]}
    >
      ...
    </Flex>
  )
}
```

### Good Pattern 2: 외부 객체 + var(--color)

```tsx
// Good - 외부 객체에서는 var(--color) 사용
const typeColors = {
  success: 'var(--success)',
  error: 'var(--error)',
  warning: 'var(--warning)',
  info: 'var(--info)',
} as const

function ToastItem({ toast }: Props) {
  return (
    <Flex bg={typeColors[toast.type]}>
      ...
    </Flex>
  )
}
```

### 권장: 인라인 객체 인덱싱

**외부 객체를 만들지 말고 인라인으로 작성하는 것이 가장 좋음:**

```tsx
// Best - 인라인 객체 인덱싱
<Box
  bg={{
    success: '$success',
    error: '$error',
    warning: '$warning',
  }[status]}
  color={{
    success: '$successText',
    error: '$errorText',
    warning: '$warningText',
  }[status]}
/>
```

### 정리

| 위치 | 문법 | 예시 |
|------|------|------|
| JSX prop에 직접 | `$color` | `bg="$primary"` |
| JSX prop 내 인라인 객체 | `$color` | `bg={{ a: '$primary' }[v]}` |
| 외부 정의 객체/변수 | `var(--color)` | `const x = { a: 'var(--primary)' }` |
| css() 함수 내부 | `$color` | `css({ bg: '$primary' })` |

### 왜 이런 차이가 있는가?

```tsx
// devup-ui 빌드 타임 변환
<Box bg="$primary" />
// → <Box bg="var(--primary)" />  (빌드 시 변환됨)

// 외부 객체는 변환 대상이 아님
const colors = { main: '$primary' }  // 그대로 '$primary' 문자열
<Box bg={colors.main} />  // '$primary' 문자열이 그대로 전달됨 (작동 안함)
```

## 26. css() Returns String (className)

> **`css()`는 string을 반환! `className`에 사용해야 하며, spread(`...`)로 펼치면 안됨!**

`css()` 함수는 스타일 객체가 아닌 **className 문자열**을 반환합니다.

### Anti-Pattern (완전히 잘못된 코드)

```tsx
// Bad - css()를 spread로 펼침 (작동 안함!)
const baseStyle = css({ py: '10px', cursor: 'pointer' })
const selectedStyle = css({ borderBottomColor: '$primary' })

export function Tab({ selected }: TabProps) {
  return (
    <Center
      {...baseStyle}                          // 틀림! string을 spread 못함
      {...(selected ? selectedStyle : {})}    // 틀림!
    >
      ...
    </Center>
  )
}
```

### Good Pattern (className 사용)

```tsx
import { css } from '@devup-ui/react'
import clsx from 'clsx'

const baseStyle = css({ py: '10px', cursor: 'pointer' })
const selectedStyle = css({ borderBottomColor: '$primary' })
const defaultStyle = css({ _hover: { borderBottomColor: '$gray300' } })

export function Tab({ selected, onClick }: TabProps) {
  return (
    <Center
      className={clsx(baseStyle, selected ? selectedStyle : defaultStyle)}
      onClick={onClick}
      styleOrder={1}
    >
      ...
    </Center>
  )
}
```

### css() vs 직접 props

| 방식 | 사용법 | 용도 |
|------|--------|------|
| **직접 props** | `<Box bg="$primary" />` | 단순 스타일 |
| **css()** | `<Box className={css({...})} />` | 재사용 스타일, variants |

### 올바른 사용 예시

```tsx
// Variant styles with css()
const buttonVariants = {
  primary: css({ bg: '$primary', color: '#FFF' }),
  secondary: css({ bg: '$gray100', color: '$text' }),
} as const

export function Button({ variant = 'primary' }: ButtonProps) {
  return (
    <Box
      as="button"
      className={buttonVariants[variant]}  // className에 사용!
      styleOrder={1}
    >
      {children}
    </Box>
  )
}

// 조건부 스타일 조합
const baseCard = css({ p: 4, borderRadius: '12px' })
const activeCard = css({ border: '2px solid $primary' })
const inactiveCard = css({ border: '1px solid $border' })

export function Card({ active }: CardProps) {
  return (
    <Box className={clsx(baseCard, active ? activeCard : inactiveCard)}>
      ...
    </Box>
  )
}
```

### styleOrder 필수

`className`과 직접 props를 함께 사용할 때 **`styleOrder={1}` 필수**:

```tsx
<Box
  className={baseStyle}     // css() 결과
  bg="$background"          // 직접 prop (이게 우선되어야 함)
  styleOrder={1}            // 필수! 직접 prop이 className보다 우선
>
```

### 주의: 'use client' 불필요

css()만 사용하고 hooks/내부 핸들러 없으면 **'use client' 필요 없음**:

```tsx
// 'use client' 없음! Server Component 가능
import { css } from '@devup-ui/react'

const tabStyle = css({ py: '10px', cursor: 'pointer' })

export function Tab({ label, selected, onClick }: TabProps) {
  return (
    <Center
      className={tabStyle}
      onClick={onClick}  // props로 받은 함수 전달만 → Server Component OK
    >
      <Text color={selected ? '$primary' : '$text'}>{label}</Text>
    </Center>
  )
}
```

## 27. No Inline style Prop

> **`style={{...}}` 사용 금지! `className={css({...})}` 사용!**

인라인 `style` prop 대신 `css()` 유틸리티로 className을 생성합니다.

### Anti-Pattern (inline style)

```tsx
// Bad - inline style 사용
<input
  ref={inputRef}
  accept="image/*"
  onChange={handleFileChange}
  style={{ display: 'none' }}  // Bad!
  type="file"
/>

// Bad - 여러 스타일
<div style={{ 
  position: 'absolute', 
  top: 0, 
  left: 0,
  background: 'rgba(0,0,0,0.5)' 
}}>
```

### Good Pattern (css() + className)

```tsx
import { css } from '@devup-ui/react'

// Good - css() 사용
<input
  ref={inputRef}
  accept="image/*"
  className={css({ display: 'none' })}
  onChange={handleFileChange}
  type="file"
/>

// Good - 여러 스타일
<div className={css({ 
  position: 'absolute', 
  top: 0, 
  left: 0,
  bg: 'rgba(0,0,0,0.5)' 
})}>
```

### 왜 style을 피하는가?

| 문제 | 설명 |
|------|------|
| **일관성** | 프로젝트 전체가 devup-ui props 또는 css() 사용 |
| **테마 토큰** | style에서는 `$primary` 같은 토큰 사용 불가 |
| **빌드 최적화** | css()는 빌드 타임에 최적화, style은 런타임 |
| **타입 안전** | css()는 타입 체크, style은 any 허용 |

### 네이티브 HTML 요소에서

devup-ui Box 대신 네이티브 요소 사용 시에도 css():

```tsx
// Good - 네이티브 input에 css() 사용
<input
  className={css({ display: 'none' })}
  type="file"
/>

// Good - 네이티브 div에 css() 사용  
<div className={css({ position: 'fixed', inset: 0 })}>
  ...
</div>
```

### 정리

| 상황 | 방법 |
|------|------|
| devup-ui 컴포넌트 | 직접 props (`<Box bg="$primary" />`) |
| 네이티브 요소 | `className={css({...})}` |
| 조건부/재사용 스타일 | `className={css({...})}` 또는 외부 정의 |
| **절대 사용 금지** | `style={{...}}` |

## 28. No Internal Handler Definition

> **내부에서 핸들러 함수를 정의하지 않고, 조건부 인라인 핸들러 사용!**

`const handleClick = ...`로 핸들러를 정의하면 `'use client'`가 필요합니다.
조건부 인라인 핸들러를 사용하면 Server Component로 유지할 수 있습니다.

### Anti-Pattern (내부 핸들러 정의)

```tsx
// Bad - 내부에서 핸들러 정의 → 'use client' 필요
'use client'

export function Radio({ checked, onChange, disabled }: RadioProps) {
  // 내부에서 함수 정의 → 'use client' 강제
  const handleClick = () => {
    if (!disabled && onChange) {
      onChange(true)
    }
  }

  return (
    <Flex onClick={handleClick}>
      ...
    </Flex>
  )
}
```

### Good Pattern (조건부 인라인 핸들러)

```tsx
// Good - 'use client' 없이 Server Component 유지!
export function Radio({ checked, onChange, disabled }: RadioProps) {
  return (
    <Flex
      // 조건부로 함수 생성 → Server Component 가능
      onClick={!disabled && onChange ? () => onChange(true) : undefined}
    >
      ...
    </Flex>
  )
}
```

### CRITICAL: 모든 조건을 && 로 결합

`disabled`, `onChange` 등 여러 조건이 있을 때 **모두 &&로 결합**해야 합니다.

```tsx
// ❌ Bad - disabled만 체크, onChange는 optional chaining
onClick={disabled ? undefined : () => onChange?.(!checked)}
// 문제: disabled가 false여도 onChange가 없으면 빈 함수 생성 → 'use client' 필요

// ✅ Good - 모든 조건을 &&로 결합
onClick={!disabled && onChange ? () => onChange(!checked) : undefined}
// 장점: disabled가 true이거나 onChange가 없으면 함수 생성 안 함 → 'use client' 불필요
```

**실전 예시 (Checkbox):**

```tsx
// ❌ Bad - 'use client' 필요
'use client'
export function Checkbox({ checked, onChange, disabled }: CheckboxProps) {
  return (
    <Center
      onClick={disabled ? undefined : () => onChange?.(!checked)}
    >
      ...
    </Center>
  )
}

// ✅ Good - 'use client' 불필요!
export function Checkbox({ checked, onChange, disabled }: CheckboxProps) {
  return (
    <Center
      onClick={!disabled && onChange ? () => onChange(!checked) : undefined}
    >
      ...
    </Center>
  )
}
```

### 복잡한 조건 처리

```tsx
// 여러 조건이 있어도 &&로 결합
<Box
  onClick={
    !disabled && !loading && onSubmit
      ? () => onSubmit(data)
      : undefined
  }
>
```

### 핸들러 내부 로직이 복잡한 경우

로직이 복잡하면 **부모에서 처리하고 콜백만 전달**:

```tsx
// Parent (Client Component)
'use client'

function ParentForm() {
  const handleRadioChange = (value: string) => {
    // 복잡한 로직은 여기서
    validate(value)
    updateState(value)
    trackAnalytics(value)
  }

  return <Radio onChange={handleRadioChange} />
}

// Child (Server Component 유지 가능!)
function Radio({ onChange }: RadioProps) {
  return (
    <Flex onClick={onChange ? () => onChange(value) : undefined}>
      ...
    </Flex>
  )
}
```

### 정리

| 패턴 | 'use client' 필요 | 권장 |
|------|------------------|------|
| `const handleClick = () => {...}` | **Yes** | ❌ |
| `onClick={() => fn?.()}` | **Yes** (항상 생성) | ❌ |
| `onClick={cond ? undefined : () => fn?.()}` | **Yes** (fn 없어도 함수 생성) | ❌ |
| `onClick={fn ? () => fn() : undefined}` | **No** | ✅ |
| `onClick={!cond && fn ? () => fn() : undefined}` | **No** | ✅ |

## 29. Direct Props Over css()

> **직접 props로 가능한 스타일은 `css()`를 사용하지 않는다!**

devup-ui 컴포넌트는 모든 스타일 속성을 props로 받을 수 있습니다. 단순한 스타일에는 직접 props를 사용하세요.

### Anti-Pattern (불필요한 css())

```tsx
// Bad - 직접 props로 가능한데 css() 사용
export function Container({ fullWidth }: Props) {
  return (
    <Box className={css({ w: fullWidth ? '100%' : 'auto' })}>
      ...
    </Box>
  )
}

// Bad - 단순 조건에 css() 사용
<Flex className={css({ alignItems: 'center', justifyContent: 'space-between' })}>
  ...
</Flex>
```

### Good Pattern (직접 props)

```tsx
// Good - 직접 props 사용
export function Container({ fullWidth }: Props) {
  return (
    <Box w={fullWidth ? '100%' : 'auto'}>
      ...
    </Box>
  )
}

// Good - 단순 스타일은 직접 props
<Flex alignItems="center" justifyContent="space-between">
  ...
</Flex>
```

### 언제 css()를 사용하는가?

| 상황 | 방법 |
|------|------|
| 단순 스타일 1-3개 | **직접 props** |
| 조건부 단일 값 | **직접 props** (`w={cond ? 'a' : 'b'}`) |
| 재사용되는 스타일 세트 | **css()** + 변수 |
| pseudo-selector가 포함된 스타일 세트 | **css()** (`_hover`, `_active` 등) |
| clsx로 조합하는 variant | **css()** |

### 실전 비교

```tsx
// ❌ Bad - 불필요한 css()
<Box className={css({ display: isVisible ? 'block' : 'none' })}>
<Box className={css({ mt: 4, mb: 2 })}>
<Flex className={css({ gap: 3 })}>

// ✅ Good - 직접 props
<Box display={isVisible ? 'block' : 'none'}>
<Box mt={4} mb={2}>
<Flex gap={3}>

// ✅ Good - css()가 필요한 경우 (pseudo-selector 포함)
const hoverCard = css({
  borderRadius: '12px',
  transition: 'all 0.2s ease',
  _hover: { boxShadow: '$lg', transform: 'translateY(-2px)' },
})

<Box className={hoverCard} p={4}>  {/* p는 직접 prop */}
```

### 핵심 원칙

```
직접 props로 가능 → 직접 props 사용
재사용/복잡한 스타일 세트 → css() 사용
```

## 30. No External Style Config Objects

> **size/variant 스타일을 외부 객체로 분리하지 않는다! 인라인 객체 인덱싱 사용!**

Section 22에서 이미 다뤘지만, 이 규칙을 더 명확히 합니다.

### Anti-Pattern (외부 config 객체)

```tsx
// Bad - 외부 객체로 스타일 분리
const sizeStyles = {
  lg: { h: '48px', fontSize: '16px', px: '24px' },
  md: { h: '40px', fontSize: '14px', px: '16px' },
  sm: { h: '32px', fontSize: '12px', px: '12px' },
}

const variantStyles = {
  primary: { bg: '$primary', color: '#FFF' },
  secondary: { bg: '$gray100', color: '$text' },
}

export function Button({ size = 'md', variant = 'primary' }: Props) {
  const sizeConfig = sizeStyles[size]  // 불필요한 중간 변수
  const variantConfig = variantStyles[variant]
  
  return (
    <Box
      h={sizeConfig.h}
      fontSize={sizeConfig.fontSize}
      px={sizeConfig.px}
      bg={variantConfig.bg}
      color={variantConfig.color}
    >
      ...
    </Box>
  )
}
```

### Good Pattern (인라인 객체 인덱싱)

```tsx
// Good - 인라인 객체 인덱싱
export function Button({ size = 'md', variant = 'primary' }: Props) {
  return (
    <Box
      h={{ lg: '48px', md: '40px', sm: '32px' }[size]}
      fontSize={{ lg: '16px', md: '14px', sm: '12px' }[size]}
      px={{ lg: '24px', md: '16px', sm: '12px' }[size]}
      bg={{ primary: '$primary', secondary: '$gray100' }[variant]}
      color={{ primary: '#FFF', secondary: '$text' }[variant]}
    >
      ...
    </Box>
  )
}
```

### 예외: css() variant 객체

css()로 className을 생성하는 경우 외부 객체 허용:

```tsx
// OK - css() variants (빌드 타임 처리)
const buttonVariants = {
  primary: css({ bg: '$primary', color: '#FFF', _hover: { bg: '$primaryBold' } }),
  secondary: css({ bg: '$gray100', color: '$text', _hover: { bg: '$gray200' } }),
} as const

export function Button({ variant = 'primary' }: Props) {
  return (
    <Box className={buttonVariants[variant]} styleOrder={1}>
      ...
    </Box>
  )
}
```

### 왜 인라인을 선호하는가?

| 외부 객체 | 인라인 객체 |
|----------|-----------|
| 스타일이 사용되는 곳에서 멀리 정의됨 | 사용 위치에서 바로 확인 가능 |
| 중간 변수 필요 (`const config = ...`) | 중간 변수 불필요 |
| 수정 시 두 곳 확인 필요 | 해당 라인만 수정 |
| 컴포넌트 파일이 길어짐 | 컴포넌트 내 코로케이션 |

### 정리

| 패턴 | 권장 |
|------|------|
| `const sizeStyles = {...}` + `sizeStyles[size].prop` | ❌ |
| `prop={{ a: 'x', b: 'y' }[variant]}` | ✅ |
| `const variants = { a: css({...}), b: css({...}) }` | ✅ (css()만) |

## 31. Static Strings Over Template Literals

> **템플릿 리터럴 내에서 동적 값을 사용하지 않는다! 완전한 정적 문자열로 작성!**

CSS 값은 최대한 정적으로 작성해야 빌드 타임 최적화가 가능합니다.

### Anti-Pattern (템플릿 리터럴에 동적 값)

```tsx
// ❌ Bad - 템플릿 리터럴 내 동적 값
<Box
  transform={
    checked
      ? `translateX(${size === 'md' ? '20px' : '16px'})`
      : 'translateX(0)'
  }
/>
// 문제: 빌드 타임에 최적화 불가, 런타임에 문자열 생성
```

### Good Pattern (완전한 정적 문자열)

```tsx
// ✅ Good - 중첩 삼항으로 정적 문자열만 사용
<Box
  transform={
    checked
      ? size === 'md'
        ? 'translateX(20px)'
        : 'translateX(16px)'
      : 'translateX(0)'
  }
/>
// 장점: 모든 값이 정적 문자열, 빌드 타임 최적화 가능
```

### 더 좋은 패턴 (인라인 객체 인덱싱)

```tsx
// ✅ Best - 인라인 객체 인덱싱
<Box
  transform={{
    md: checked ? 'translateX(20px)' : 'translateX(0)',
    sm: checked ? 'translateX(16px)' : 'translateX(0)',
  }[size]}
/>
```

### 비교

| 패턴 | 권장 |
|------|------|
| `` `translateX(${dynamicValue})` `` | ❌ |
| `cond ? 'translateX(20px)' : 'translateX(16px)'` | ✅ |
| `{{ a: 'translateX(20px)', b: 'translateX(16px)' }[size]}` | ✅ Best |

## 32. No Intermediate Style Variables

> **size/variant 기반 스타일을 중간 변수로 추출하지 않는다!**

컴포넌트 함수 내에서 `const width = ...` 형태로 스타일 값을 추출하지 않습니다.

### Anti-Pattern (중간 변수 추출)

```tsx
// ❌ Bad - 불필요한 중간 변수
export function Toggle({ size = 'md', checked, disabled, onChange }: ToggleProps) {
  const width = size === 'md' ? '44px' : '36px'
  const height = size === 'md' ? '24px' : '20px'
  const thumbSize = size === 'md' ? '20px' : '16px'

  return (
    <Flex
      w={width}
      h={height}
      // ...
    >
      <Box
        w={thumbSize}
        h={thumbSize}
        transform={
          checked
            ? size === 'md'
              ? 'translateX(20px)'
              : 'translateX(16px)'
            : 'translateX(0)'
        }
      />
    </Flex>
  )
}
// 문제: 불필요한 변수 선언, 코드 분산, 수정 시 두 곳 확인 필요
```

### Good Pattern (인라인 객체 인덱싱)

```tsx
// ✅ Good - 인라인 객체 인덱싱으로 직접 사용
export function Toggle({ size = 'md', checked, disabled, onChange }: ToggleProps) {
  return (
    <Flex
      w={{ md: '44px', sm: '36px' }[size]}
      h={{ md: '24px', sm: '20px' }[size]}
      // ...
    >
      <Box
        boxSize={{ md: '20px', sm: '16px' }[size]}
        transform={{
          md: checked ? 'translateX(20px)' : 'translateX(0)',
          sm: checked ? 'translateX(16px)' : 'translateX(0)',
        }[size]}
      />
    </Flex>
  )
}
// 장점: 스타일이 사용되는 위치에 정의, 변수 없음, 수정 용이
```

### 왜 중간 변수를 피하는가?

| 중간 변수 | 인라인 객체 |
|----------|-----------|
| 변수 선언 → 사용처로 점프 필요 | 사용처에서 바로 값 확인 |
| 변수명 고민 필요 | 변수명 불필요 |
| 코드 라인 수 증가 | 컴팩트한 코드 |
| 재사용하지 않는 값에 변수 낭비 | 필요한 곳에만 값 존재 |

### 정리

| 패턴 | 권장 |
|------|------|
| `const width = size === 'md' ? '44px' : '36px'` → `w={width}` | ❌ |
| `w={{ md: '44px', sm: '36px' }[size]}` | ✅ |
| `w={size === 'md' ? '44px' : '36px'}` (단순 삼항) | ⚠️ OK (2개 옵션일 때만) |