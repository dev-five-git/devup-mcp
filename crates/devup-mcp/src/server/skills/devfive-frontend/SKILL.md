---
name: devfive-frontend
description: |
  DevFive frontend project coding conventions and architecture patterns for Next.js App Router projects.
  
  TRIGGER WHEN:
  - Working on DevFive frontend projects (puzzle-fit, girok-space, etc.)
  - Writing React/Next.js components with devup-ui styling
  - Questions about frontend coding conventions, component structure
  - Using @devup-ui/react, @devup-api/fetch, react-hook-form
  
  Trigger keywords: frontend, React, Next.js, devup-ui, component, coding style, 
  coding convention, DevFive, puzzle-fit, girok-space
---

# DevFive Frontend Coding Conventions

Common coding conventions for DevFive frontend projects (puzzle-fit, girok-space, etc.).

## Prerequisites

> **CRITICAL: Load required skills mentioned here.**

| Skill | Description | Load Command |
|-------|-------------|--------------|
| **devup-ui** | Styling syntax, spacing scale, responsive arrays, pseudo selectors | `/devup-ui` |

**Load order:**
1. Load `/devup-ui` skill first
2. Then apply this skill (`/devfive-frontend`)

## Tech Stack

| Category | Technology |
|----------|------------|
| Framework | Next.js (App Router) 16+ |
| UI Library | React 19+ |
| Styling | @devup-ui/react, @devup-ui/components |
| API Client | @devup-api/fetch, @devup-api/react-query |
| Forms | react-hook-form 7+ |
| State | Zustand (when needed) |
| Animation | framer-motion 12+ |
| Linting | eslint-plugin-devup |
| Package Manager | **bun** (default) |

## Identify the Framework Before You Write Anything

> **CRITICAL: `vite.config.ts` in the app does NOT mean this is a Vite SPA.**

`vinext` runs Next.js App Router **on Vite** — it is a drop-in replacement for
the `next` CLI, so a perfectly normal App Router project has a `vite.config.ts`
and no `next.config.ts`. Read `package.json`, not the config filename:

```jsonc
// apps/front/package.json -> this is a Next App Router project
{ "dependencies": { "vinext": "1.0.0-beta.9" } }
```

```ts
// apps/front/vite.config.ts -> still App Router. Both plugins belong here.
plugins: [
  DevupUI(),
  vinext({ nextConfig: { output: 'export', trailingSlash: true } }),
]
```

| Signal | Verdict |
|--------|---------|
| `vinext` in dependencies | **Next App Router.** Use `src/app/` file routing |
| `next` in dependencies | **Next App Router.** Use `src/app/` file routing |
| `vite.config.ts` exists | Says nothing on its own — check dependencies |
| `@devup-ui/vite-plugin` in use | Says nothing on its own — vinext uses it too |

**Never do this in a vinext/Next project:**

```tsx
// WRONG - hand-rolled SPA routing. This is what an unrecognized framework
// looks like from the inside: one entry file swapping screens on state.
// src/main.tsx + index.html
const [screen, setScreen] = useState<Screen>('home')
return screen === 'home' ? <Home /> : <Settings />
```

```tsx
// CORRECT - one route per screen, the framework owns navigation
// src/app/page.tsx, src/app/settings/page.tsx, ...
import Link from 'next/link'
```

There is no `main.tsx` and no `index.html` in a Next App Router app. If you are
creating one, you have misread the project.

## Never Author a CSS File

> **CRITICAL: no `.css` or `.scss` file belongs in application source.**

devup-ui extracts styling at build time. A hand-written stylesheet is invisible
to it, so it cannot be checked, themed, or overridden in cascade order — and it
silently competes with the classes devup-ui generated.

| Need | Use |
|------|-----|
| Reset / normalize | `resetCss()` from `@devup-ui/reset-css` |
| Document-level rules (`body`, `*`, font-face) | `globalCss({ ... })` |
| Component styling | Style props on `Box`/`Flex`/`Text`, or `css({ ... })` |
| One-off dynamic value | A style prop — devup-ui emits a CSS variable for it |

The only CSS import that is allowed is a stylesheet **shipped by an installed
package** that you do not author — an offline webfont package, for example.

`@devup-ui/reset-css` needs **no plugin configuration**. Install it, call it in
the root layout, and stop:

```tsx
// apps/front/src/app/layout.tsx
import { resetCss } from '@devup-ui/reset-css'

resetCss()
```

The reset is a `globalCss()` call at the top level of that package's own module,
and `resetCss()` is an empty function that only keeps the import from being
tree-shaken. The one thing that has to happen is that the plugin transforms the
package inside `node_modules`, and every devup-ui plugin already allows
`@devup-ui` through that exclusion unconditionally.

Do not add `include`, `optimizeDeps.exclude` or `ssr.noExternal` entries for it.
They are redundant, and writing them teaches the next reader that a devup-ui
package needs wiring when none does.

### What decides static extraction

One rule explains every case below:

> devup-ui extracts at build time only what it can prove is constant **at the
> JSX prop site**. A value reached through a variable is treated as possibly
> mutated at runtime — TypeScript's types are not a runtime guarantee — so it
> falls back to a CSS variable.

| Form | Result |
|------|--------|
| `<Box color="red" />` | Static class |
| `<Box color={{ 1: 'red', 2: 'blue' }[v]} />` | Static class per value — **preferred** |
| `<Box color={colors[v]} />` where `colors` is declared elsewhere | CSS variable |
| `<Box color={props.color} />` | CSS variable (genuinely dynamic — correct) |
| `const s = { primary: css({ ... }) }` then `className={s[v]}` | **Fine.** See below |

```tsx
// PREFERRED - the possible values are visible at the prop, so each becomes
// its own class and nothing has to be carried at runtime.
<Box
  color={{ primary: '$primary', danger: '$red' }[variant]}
  h={{ sm: '32px', md: '40px', lg: '48px' }[size]}
/>

// AVOID - the compiler cannot prove `colors` is not mutated before this runs
const colors = { red: '#f00', blue: '#00f' }
<Box color={colors[tone]} />
```

**How to detect that you fell into the dynamic path:** inspect the rendered
element. Inline CSS variables mean the values were not extracted.

```html
<!-- extracted -->      <div class="m n o p">
<!-- not extracted -->  <div class="m n o p" style="--q:red;--s:12px">
```

### The `css()` exception

An external object holding **`css()` results** is not the same thing and is
fine. `css()` runs at build time and returns a plain className string, so the
extraction already happened at the `css()` call; the object only carries
strings, and no style prop is involved.

```tsx
// FINE - extraction happened inside css(); this object holds classNames
const variantStyles = {
  primary: css({ bg: '$primary', color: '#FFF' }),
  secondary: css({ bg: '$gray100', color: '$text' }),
}
<Box className={variantStyles[variant]} styleOrder={1} />
```

The rule is about **style prop values**, not about objects. "No external style
objects" below means no external object of style *values*.

## Project Structure

DevFive projects use **monorepo structure**:

```
project-root/
├── apps/                   # Frontend applications
│   ├── front/              # Main web app
│   ├── admin/              # Admin panel
│   └── app/                # React Native (Expo)
├── apis/                   # Backend services
├── shared/                 # Shared packages
├── package.json            # Root (includes lint:fix command)
└── bun.lock                # bun usage indicator
```

### Frontend App Structure (apps/front/src/)

```
src/
├── app/                    # Next.js App Router
│   ├── (auth)/             # Route Groups (auth required)
│   ├── (public)/           # Route Groups (public)
│   ├── layout.tsx          # Root layout
│   └── page.tsx
├── components/
│   ├── common/             # Shared components (multi-page use)
│   ├── layout/             # Layout components (Header, Footer, etc.)
│   ├── pages/              # Page-specific components
│   │   ├── my-page/        # /my-page only
│   │   ├── login/          # /login only
│   │   └── home/           # / (home) only
│   └── provider.tsx        # Global Provider composition
├── contexts/               # Context definitions
├── hooks/                  # Custom hooks
├── stores/                 # Zustand stores
├── utils/                  # Utility functions
└── api.ts                  # API client setup
```

### Component Location Rules

| Scope | Location | Example |
|-------|----------|---------|
| **app/ folder** | **Only `layout.tsx`, `page.tsx`** | No other component files! |
| Multi-page shared | `components/` or `components/common/` | `Button.tsx`, `Modal.tsx` |
| Layout related | `components/layout/` | `Header.tsx`, `Footer.tsx` |
| Page-specific | `components/pages/{page-path}/` | `pages/my-page/ProfileCard.tsx` |
| **Single-color SVG icons** | `public/icons/` (as `.svg` file) | `icons/arrow-right.svg` |

> **CRITICAL: `src/app/` 폴더에는 `layout.tsx`와 `page.tsx`만 존재해야 함!**
> 다른 컴포넌트는 `src/components/` 내에 위치.

### SVG Icon Rule

**Single-color SVG → `public/icons/*.svg`, NOT a React component.**

```tsx
// Good - single-color SVG as file
// public/icons/arrow-right.svg
<svg viewBox="0 0 24 24" fill="currentColor">
  <path d="M12 4l-1.41 1.41L16.17 11H4v2h12.17l-5.58 5.59L12 20l8-8z"/>
</svg>

// Usage in component
<Image src="/icons/arrow-right.svg" alt="arrow" width={24} height={24} />

// Bad - single-color SVG as React component (unnecessary JS bundle)
// components/icons/ArrowRightIcon.tsx
export function ArrowRightIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor">
      <path d="M12 4l-1.41 1.41L16.17 11H4v2h12.17l-5.58 5.59L12 20l8-8z"/>
    </svg>
  )
}
```

**When to use React component for SVG:**
- Multi-color SVG with dynamic color props
- SVG with animation/interaction logic
- SVG that changes based on state

## Critical Rules Summary

> **Full details: See [references/critical-rules.md](references/critical-rules.md)**

### Server Component First (CRITICAL)

Maximize Server Components to minimize JS bundle size.

**'use client' Decision Guide:**

| Feature | 'use client'? |
|---------|--------------|
| Static UI rendering | No |
| Data fetch (async/await) | No |
| `useState`, `useEffect` | **Yes** |
| Event handlers **defined internally** | **Yes** |
| Event handlers **received via props** | **No** |
| `useRouter`, `usePathname` | **Yes** |
| `framer-motion` animation | **Yes** |

**Key insight:** Components receiving optional functions via props don't need `'use client'` - but **only if you pass props directly without wrapping in arrow function**.

```tsx
// NO 'use client' needed - direct prop pass
interface CardProps {
  onClick?: () => void
}

export function Card({ onClick }: CardProps) {
  // Good patterns (can stay Server Component)
  return <Box onClick={onClick}>Content</Box>
  // or: onClick={onClick ? onClick : undefined}
  // or: onClick={onClick ? () => onClick() : undefined}
}

// ANTI-PATTERN - forces 'use client' due to arrow function
export function Card({ onClick }: CardProps) {
  return <Box onClick={() => onClick?.()}>Content</Box>  // BAD!
  // or: onClick={() => onClick ? onClick() : undefined}  // BAD!
}

// NEEDS 'use client' - defines handler internally
'use client'
export function Card() {
  const handleClick = () => console.log('clicked')
  return <Box onClick={handleClick}>Content</Box>
}
```

**Why?** Wrapping in arrow function (`() => ...`) means you're **defining** a new function, which requires `'use client'`. Direct prop pass doesn't define anything.

### Export Rules

- **One component per file** - Each file exports exactly one component
- `export default` **only in `page.tsx`**
- Page components: descriptive name + `Page` suffix (e.g., `LoginPage`, not `Page`)
- No `'use client'` in `page.tsx`

### Required Verification

```bash
bun tsc --noEmit && bun run lint:fix && bun run lint
```

### Dev Server

```bash
# Good
cd apps/front && bun dev

# Bad (zombie process risk)
bun -F front dev
```

## File Naming Conventions

| Type | Pattern | Example |
|------|---------|---------|
| Components | PascalCase | `CommonButton.tsx` |
| Hooks | camelCase + use prefix | `useBookmark.ts` |
| Utils | camelCase | `formatPrice.ts` |
| Context | PascalCase + Context | `ToastContext.tsx` |
| Page | `page.tsx` | `app/(auth)/my-page/page.tsx` |

## Component Structure

```tsx
'use client'  // Only when necessary!

// 1. React/Next.js imports
import { useState } from 'react'
import Link from 'next/link'

// 2. Third-party imports
import { AnimatePresence } from 'framer-motion'

// 3. devup-ui imports
import { Box, Flex, Text, VStack, css } from '@devup-ui/react'

// 4. Internal imports with @ aliases
import { useToastContext } from '@/contexts/ToastContext'

// 5. Relative imports
import { ChildComponent } from './ChildComponent'

// 6. Interface definitions
interface ButtonProps {
  variant: 'primary' | 'secondary'
  children: React.ReactNode
}

// 7. Style objects
const variantStyles = {
  primary: css({ bg: '$primary', color: '#FFF' }),
  secondary: css({ bg: '$gray100', color: '$text' }),
}

// 8. Named export (default export only in page.tsx!)
export function Button({ variant, children }: ButtonProps) {
  return (
    <Box className={variantStyles[variant]} styleOrder={1}>
      {children}
    </Box>
  )
}
```

## Page Component Pattern

```tsx
// app/(auth)/my-page/page.tsx
// No 'use client'! Page is Server Component

import { Suspense } from 'react'
import { Header } from '@/components/layout/Header'
import { MyPageContent } from '@/components/pages/my-page/MyPageContent'
import { Loading } from '@/components/common/Loading'

export default function MyPagePage() {  // Name must end with Page
  return (
    <VStack>
      <Header title="My Page" />
      <Suspense fallback={<Loading />}>
        <MyPageContent />
      </Suspense>
    </VStack>
  )
}
```

## Context Provider Pattern

```tsx
// contexts/ToastContext.tsx
'use client'
import { createContext, ReactNode, useCallback, useContext, useState } from 'react'

interface ToastContextValue {
  showToast: (config: { type: 'success' | 'error'; message: string }) => void
}

const ToastContext = createContext<ToastContextValue | undefined>(undefined)

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastConfig[]>([])

  const showToast = useCallback((config) => {
    const id = Math.random().toString(36).substring(2, 9)
    setToasts((prev) => [...prev, { ...config, id }])
  }, [])

  return (
    <ToastContext.Provider value={{ showToast }}>
      {children}
    </ToastContext.Provider>
  )
}

export function useToastContext() {
  const context = useContext(ToastContext)
  if (context === undefined) {
    throw new Error('useToastContext must be used within a ToastProvider')
  }
  return context
}
```

## API Integration

```tsx
// Server Component - direct fetch (recommended)
async function getData() {
  const res = await fetch('https://api.example.com/data')
  return res.json()
}

export default async function DataPage() {
  const data = await getData()
  return <DataDisplay data={data} />
}

// Client Component - react-query
'use client'
import { queryApi } from '@/api'

export function DataList() {
  const { data, isError, isPending } = queryApi.useQuery('get', '/items')
  
  if (isPending) return <Loading />
  if (isError) return <ErrorMessage />
  
  return <ItemList items={data} />
}
```

## Form Pattern

```tsx
'use client'
import { FormProvider, useForm } from 'react-hook-form'

interface FormData {
  email: string
  password: string
}

export function LoginForm() {
  const methods = useForm<FormData>()
  
  return (
    <FormProvider {...methods}>
      <VStack as="form" gap={4} onSubmit={methods.handleSubmit(onSubmit)}>
        <FormField name="email">
          <Input placeholder="Email" />
        </FormField>
        <Button type="submit">Login</Button>
      </VStack>
    </FormProvider>
  )
}
```

## ESLint Configuration

```js
// eslint.config.mjs
import { configs } from 'eslint-plugin-devup'

export default [
  { ignores: ['node_modules/**/*', '.next/**/*'] },
  ...configs.recommended,
]
```

## Quick Rules Summary

| Rule | Requirement |
|------|-------------|
| React Compiler | `reactCompiler: true` in next.config.ts |
| SEO | `metadata` with `openGraph` required |
| Image alt | Always required |
| Interactive states | `_hover`, `_active`, `cursor`, `transition` |
| Query states | Handle `isPending`, `isError` |
| Tests | Required for non-async components |
| JSDoc | Required for components |
| Types | `interface` over `type` |
| Functions | Regular function over arrow function |
| Navigation | `<Link>` over `useRouter` for simple nav |
| **HTML tag** | `suppressHydrationWarning` required (theme) |
| **Footer background** | body bg = footer bg, content gets own bg |
| **as const** | Use for object/array literals without explicit type |
| **Inline variant style** | `{{ sm: '36px', md: '44px' }[size]}`, `typography`만 `as const` 필수 |
| **$color vs var(--color)** | `$color` in JSX prop only, `var(--color)` in external objects |
| **css() returns string** | Use `className={css(...)}`, NOT `{...css(...)}` |
| **No inline style** | Use `className={css({...})}`, NOT `style={{...}}` |
| **No internal handler** | `onClick={!disabled && onChange ? () => onChange() : undefined}` |
| **Direct props over css()** | `w={fullWidth ? '100%' : 'auto'}`, NOT `className={css({...})}` |
| **No external style objects** | No external object of style *values* (`{red:'#f00'}`). An object of `css()` classNames is fine |
| **Framework** | `vinext` in deps = Next App Router. `vite.config.ts` proves nothing. No `main.tsx`/`index.html` |
| **No authored CSS** | No `.css`/`.scss` in source. `resetCss()` + `globalCss()` + style props |
| **No barrel files** | Don't create `index.tsx` just for re-exports |
| **Avoid closure in props** | Pass value to child, let child handle callback |
| **Static strings only** | `'translateX(20px)'` NOT `` `translateX(${val})` `` |
| **No intermediate variables** | `w={{ md: '44px' }[size]}` NOT `const w = ...; w={w}` |
| **app/ folder** | Only `layout.tsx`, `page.tsx` allowed |
| **Isolate client hooks** | Extract hook-dependent parts to separate Client Component |

## References

- **[Critical Rules (detailed)](references/critical-rules.md)** - Full rule explanations with examples
- **[Anti-Patterns](references/anti-patterns.md)** - What NOT to do
- **[Theme Colors](references/theme-colors.md)** - Project theme color tokens
- **[Common Patterns](references/common-patterns.md)** - Common UI patterns (Modal, Toast, etc.)
