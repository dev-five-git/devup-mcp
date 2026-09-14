# Anti-Patterns (NEVER do)

## Quick Reference Table

| Wrong | Right | Why |
|-------|-------|-----|
| `bun -F front dev` | `cd apps/front && bun dev` | Prevent zombie processes |
| `reactCompiler: false` | `reactCompiler: true` | React Compiler required |
| metadata without `openGraph` | Include `openGraph: {...}` | OpenGraph required |
| `<Image>` without alt | `<Image alt="...">` | Accessibility required |
| Button without `_hover`, `_active` | Add interactive states | UX required |
| Clickable without `cursor` | `cursor="pointer"` | Indicate clickability |
| No `transition` on interactive | `transition="all 0.2s ease"` | Smooth UX |
| Query without `isPending` check | `if (isPending) return <Loading />` | Loading UI required |
| Query without `isError` check | `if (isError) return <Error />` | Error UI required |
| Component without test | `__tests__/Component.test.tsx` | Testing required (except async) |
| Component without JSDoc | `/** @description ... */` | Documentation required |
| `export default` in non-page | Named export | default export only in page.tsx |
| `export default function Page()` | `LoginPage`, `DashboardPage` etc | Descriptive name required |
| `'use client'` in page.tsx | Remove it | Page is Server Component |
| `'use client'` overuse | Minimize separation | Minimize JS bundle size |
| Entire component as Client | Use children pattern | Keep Server Components |
| Layout as separate Client component | Place directly in page.tsx | Remove unnecessary JS |
| `async` without `await` in RSC | Remove `async` | Remove unnecessary async |
| `<><Child/></>` (1 child) | Remove fragment | Remove Fragment if 1 child |
| `<Box style={{...}}>` | `<Box bg="...">` | style prop not extracted |
| Skip type check | `bun tsc --noEmit` | Type check required |
| Skip lint | `bun run lint:fix && bun run lint` | Lint required |
| Create custom Checkbox/Select/etc. | Use `@devup-ui/components` | Use existing components |
| `type Props = {...}` | `interface Props {...}` | Prefer interface |
| `const Comp = () => {}` | `function Comp() {}` | Prefer regular function |
| Unused params/variables | Remove them | Remove unnecessary code |
| `<a href="/path">` for internal | `<Link href="/path">` | Next.js Link required |
| `useRouter` for simple nav | `<Link href="/path">` | Link works in Server Component, useRouter needs 'use client' |
| Skip loading required skills | Load `/devup-ui` first | Load required skills first |
| `onClick={() => onClick?.()}` | `onClick={onClick}` | Arrow wrapper forces 'use client', direct pass doesn't |
| `onClick={() => fn ? fn() : undefined}` | `onClick={fn ? fn : undefined}` | Arrow wrapper forces 'use client' |
| `onClick={disabled ? undefined : () => fn?.()}` | `onClick={!disabled && fn ? () => fn() : undefined}` | Optional chaining still creates function |
| `` `translateX(${size === 'md' ? '20px' : '16px'})` `` | `size === 'md' ? 'translateX(20px)' : 'translateX(16px)'` | Static strings enable build optimization |
| `const w = size === 'md' ? '44px' : '36px'; w={w}` | `w={{ md: '44px', sm: '36px' }[size]}` | No intermediate variables |
| `typography={{ a: 'x', b: 'y' }[key]}` | `typography={({ a: 'x', b: 'y' } as const)[key]}` | typography requires literal types |
| Component files in `app/` folder | Only `layout.tsx`, `page.tsx` in app/ | Components go to `src/components/` |
| Entire component Client for hooks | Extract only hook-dependent parts | Minimize Client Component scope |

## Detailed Examples

### 'use client' Overuse

```tsx
// WRONG - entire component as client
'use client'
export function ProductPage() {
  const [count, setCount] = useState(0)
  return (
    <VStack>
      <ProductInfo />      {/* Now unnecessarily client */}
      <ProductReviews />   {/* Now unnecessarily client */}
      <AddButton count={count} onClick={() => setCount(c => c + 1)} />
    </VStack>
  )
}

// RIGHT - minimal client separation
// page.tsx (Server Component)
export default function ProductPage() {
  return (
    <VStack>
      <ProductInfo />      {/* Server Component */}
      <ProductReviews />   {/* Server Component */}
      <AddButtonWrapper /> {/* Only this is Client */}
    </VStack>
  )
}

// AddButtonWrapper.tsx
'use client'
export function AddButtonWrapper() {
  const [count, setCount] = useState(0)
  return <AddButton count={count} onClick={() => setCount(c => c + 1)} />
}
```

### Props-based Functions (CRITICAL)

The key is **how you pass the prop**, not whether you receive it.

**Good patterns (can stay Server Component):**

```tsx
interface CardProps {
  onClick?: () => void
}

// Direct pass - OK
export function Card({ onClick }: CardProps) {
  return <Box onClick={onClick}>Content</Box>
}

// Conditional direct pass - OK
export function Card({ onClick }: CardProps) {
  return <Box onClick={onClick ? onClick : undefined}>Content</Box>
}

// Conditional wrapper (only creates when exists) - OK
export function Card({ onClick }: CardProps) {
  return <Box onClick={onClick ? () => onClick() : undefined}>Content</Box>
}
```

**Anti-patterns (forces 'use client'):**

```tsx
// WRONG - arrow function ALWAYS creates a new function
export function Card({ onClick }: CardProps) {
  return <Box onClick={() => onClick?.()}>Content</Box>
}

// WRONG - same problem
export function Card({ onClick }: CardProps) {
  return <Box onClick={() => onClick ? onClick() : undefined}>Content</Box>
}
```

**Why?** `onClick={() => ...}` always creates a new arrow function, which means you're **defining** a handler internally. This forces `'use client'`.

Direct pass (`onClick={onClick}`) just passes the reference - no new function is defined, so no `'use client'` needed.

### Combined Conditions with && (CRITICAL)

When you have multiple conditions (disabled, onChange, etc.), combine them ALL with `&&`.

```tsx
// WRONG - disabled만 체크하고 onChange는 optional chaining
'use client'  // 불필요!
export function Checkbox({ checked, onChange, disabled }: CheckboxProps) {
  return (
    <Center
      onClick={disabled ? undefined : () => onChange?.(!checked)}
    >
      ...
    </Center>
  )
}
// 문제: disabled=false 일 때, onChange가 없어도 () => undefined?.(!checked) 함수가 생성됨

// RIGHT - 모든 조건을 &&로 결합
export function Checkbox({ checked, onChange, disabled }: CheckboxProps) {
  return (
    <Center
      onClick={!disabled && onChange ? () => onChange(!checked) : undefined}
    >
      ...
    </Center>
  )
}
// 장점: disabled=true 이거나 onChange가 없으면 함수 생성 안 함 → 'use client' 불필요
```

**Key insight:** `() => fn?.()` ALWAYS creates a function, even if `fn` is undefined. Use `fn ? () => fn() : undefined` instead.

### Interactive States Missing

```tsx
// WRONG - missing states
<Box as="button" bg="$primary" color="#FFF">
  Click me
</Box>

// RIGHT - all states included
<Box
  as="button"
  bg="$primary"
  color="#FFF"
  cursor="pointer"
  transition="all 0.2s ease"
  _hover={{ bg: '$primaryBold' }}
  _active={{ bg: '$primaryExBold', transform: 'scale(0.98)' }}
  _disabled={{ bg: '$gray200', color: '$gray400', cursor: 'not-allowed' }}
>
  Click me
</Box>
```

### Query State Handling

```tsx
// WRONG - no state handling
export function UserList() {
  const { data } = queryApi.useQuery('get', '/users')
  return data?.map(user => <UserCard key={user.id} user={user} />)
}

// RIGHT - all states handled
export function UserList() {
  const { data, isError, isPending } = queryApi.useQuery('get', '/users')
  
  if (isPending) return <Loading />
  if (isError) return <ErrorMessage />
  if (!data?.length) return <EmptyState message="No users" />
  
  return data.map(user => <UserCard key={user.id} user={user} />)
}
```

### Navigation

```tsx
// WRONG - useRouter for simple navigation
'use client'
export function NavButton() {
  const router = useRouter()
  return <button onClick={() => router.push('/about')}>About</button>
}

// RIGHT - Link (Server Component compatible)
import Link from 'next/link'

export function NavButton() {
  return <Link href="/about"><Button>About</Button></Link>
}
```

### Template Literals with Dynamic Values

```tsx
// WRONG - 템플릿 리터럴 내 동적 값
<Box
  transform={
    checked
      ? `translateX(${size === 'md' ? '20px' : '16px'})`
      : 'translateX(0)'
  }
/>

// RIGHT - 완전한 정적 문자열
<Box
  transform={
    checked
      ? size === 'md'
        ? 'translateX(20px)'
        : 'translateX(16px)'
      : 'translateX(0)'
  }
/>

// BEST - 인라인 객체 인덱싱
<Box
  transform={{
    md: checked ? 'translateX(20px)' : 'translateX(0)',
    sm: checked ? 'translateX(16px)' : 'translateX(0)',
  }[size]}
/>
```

### Intermediate Style Variables

```tsx
// WRONG - 불필요한 중간 변수
export function Toggle({ size = 'md', checked, disabled, onChange }: ToggleProps) {
  const width = size === 'md' ? '44px' : '36px'
  const height = size === 'md' ? '24px' : '20px'
  const thumbSize = size === 'md' ? '20px' : '16px'

  return (
    <Flex w={width} h={height}>
      <Box w={thumbSize} h={thumbSize} />
    </Flex>
  )
}

// RIGHT - 인라인 객체 인덱싱
export function Toggle({ size = 'md', checked, disabled, onChange }: ToggleProps) {
  return (
    <Flex
      w={{ md: '44px', sm: '36px' }[size]}
      h={{ md: '24px', sm: '20px' }[size]}
    >
      <Box boxSize={{ md: '20px', sm: '16px' }[size]} />
    </Flex>
  )
}
```

### Component Files in app/ Folder

```
// WRONG - app/ 폴더에 컴포넌트 파일 존재
src/app/
├── layout.tsx
├── page.tsx
├── DetailPageContent.tsx   ❌ 컴포넌트 파일!
└── (auth)/
    ├── page.tsx
    └── LoginForm.tsx       ❌ 컴포넌트 파일!

// RIGHT - 컴포넌트는 src/components/ 내에 위치
src/app/
├── layout.tsx
├── page.tsx
└── (auth)/
    └── page.tsx

src/components/
├── common/
│   └── Button.tsx          ✅
├── pages/
│   ├── home/
│   │   └── HomeContent.tsx ✅
│   └── auth/
│       └── LoginForm.tsx   ✅
└── layout/
    └── Header.tsx          ✅
```

### Client Hook Isolation (CRITICAL)

`'use client'`가 필요한 hook (`useRouter`, `useState`, `useEffect` 등)을 사용할 때,
**hook이 필요한 부분만 최소 범위로 분리**합니다.

```tsx
// WRONG - hook 때문에 전체가 Client Component
'use client'
export function DetailPageContent({ id }: { id: string }) {
  const router = useRouter()

  return (
    <>
      <PageHeader onBack={() => router.back()} title={`Item #${id}`} />
      <VStack gap="30px">
        {/* 많은 정적 UI... 전부 불필요하게 Client */}
        <Table>...</Table>
        <Table>...</Table>
      </VStack>
      <Flex>
        <Button onClick={() => router.back()}>Cancel</Button>
        <Button>Save</Button>
      </Flex>
    </>
  )
}

// RIGHT - hook 사용 부분만 분리
// DetailPageContent.tsx (Server Component)
export function DetailPageContent({ id }: { id: string }) {
  return (
    <>
      <PageHeaderWithBack title={`Item #${id}`} />
      <VStack gap="30px">
        <Table>...</Table>  {/* Server Component 유지! */}
        <Table>...</Table>
      </VStack>
      <DetailPageFooter />
    </>
  )
}

// PageHeaderWithBack.tsx (Client Component - 최소 범위)
'use client'
export function PageHeaderWithBack({ title }: { title: string }) {
  const router = useRouter()
  return <PageHeader onBack={() => router.back()} title={title} />
}

// DetailPageFooter.tsx (Client Component - 최소 범위)
'use client'
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

**핵심:** hook이 필요한 부분만 최소 범위로 추출! 나머지 정적 UI는 Server Component로 유지해 JS 번들 최소화.
