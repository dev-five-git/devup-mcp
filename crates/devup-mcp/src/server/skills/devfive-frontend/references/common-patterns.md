# Common Patterns Reference

데브파이브 프론트엔드 프로젝트에서 자주 사용되는 UI 패턴들입니다.

## 1. Global Provider Pattern

`provider.tsx`에서 모든 글로벌 Provider를 조합합니다:

```tsx
// src/components/provider.tsx
import { BottomSheetProvider } from '@/contexts/BottomSheetContext'
import { LoadingProvider } from '@/contexts/LoadingContext'
import { ModalProvider } from '@/contexts/ModalContext'
import { ToastProvider } from '@/contexts/ToastContext'

export function Provider({ children }: { children: React.ReactNode }) {
  return (
    <ModalProvider>
      <BottomSheetProvider>
        <LoadingProvider>
          <ToastProvider>{children}</ToastProvider>
        </LoadingProvider>
      </BottomSheetProvider>
    </ModalProvider>
  )
}
```

## 2. Modal Pattern

### Context 기반 Modal

```tsx
// contexts/ModalContext.tsx
'use client'
import { createContext, ReactNode, useCallback, useContext, useState } from 'react'
import { Modal } from '@/components/layout/modal'

interface ModalConfig {
  title: ReactNode
  description?: ReactNode
  onOk?: () => void | Promise<void>
  onClose?: () => void
  okButton?: ReactNode
  closeButton?: ReactNode
}

interface ModalContextValue {
  openModal: (config: ModalConfig) => void
  closeModal: () => void
}

const ModalContext = createContext<ModalContextValue | undefined>(undefined)

export function ModalProvider({ children }: { children: ReactNode }) {
  const [modalConfig, setModalConfig] = useState<ModalConfig | null>(null)

  const openModal = useCallback((config: ModalConfig) => {
    setModalConfig(config)
  }, [])

  const closeModal = useCallback(() => {
    setModalConfig(null)
  }, [])

  return (
    <ModalContext.Provider value={{ openModal, closeModal }}>
      {children}
      <Modal
        open={!!modalConfig}
        title={modalConfig?.title ?? ''}
        description={modalConfig?.description}
        onClose={() => {
          modalConfig?.onClose?.()
          closeModal()
        }}
        onOk={async () => {
          await modalConfig?.onOk?.()
          closeModal()
        }}
      />
    </ModalContext.Provider>
  )
}

export function useModalContext() {
  const context = useContext(ModalContext)
  if (context === undefined) {
    throw new Error('useModalContext must be used within a ModalProvider')
  }
  return context
}
```

### 사용 예시

```tsx
const { openModal } = useModalContext()

openModal({
  title: '삭제하시겠습니까?',
  description: '이 작업은 되돌릴 수 없습니다.',
  onOk: async () => {
    await deleteItem()
  },
})
```

## 3. Toast Pattern

```tsx
// contexts/ToastContext.tsx
'use client'
import { createContext, ReactNode, useCallback, useContext, useState } from 'react'

export interface ToastConfig {
  id: string
  type: 'success' | 'error' | 'warning'
  message: string
  duration?: number
}

interface ToastContextValue {
  showToast: (config: Omit<ToastConfig, 'id'>) => void
}

const ToastContext = createContext<ToastContextValue | undefined>(undefined)

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastConfig[]>([])

  const showToast = useCallback((config: Omit<ToastConfig, 'id'>) => {
    const id = Math.random().toString(36).substring(2, 9)
    setToasts((prev) => [...prev, { ...config, id }])
    
    // Auto dismiss
    setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id))
    }, config.duration ?? 3000)
  }, [])

  return (
    <ToastContext.Provider value={{ showToast }}>
      {children}
      {/* Toast UI 렌더링 */}
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

### 사용 예시

```tsx
const { showToast } = useToastContext()

showToast({
  type: 'success',
  message: '저장되었습니다.',
  duration: 2000,
})
```

## 4. BottomSheet Pattern

```tsx
// components/layout/bottom-sheet/index.tsx
'use client'
import { Box, Center, css } from '@devup-ui/react'
import { AnimatePresence } from 'framer-motion'
import { useState } from 'react'
import { MotionDiv } from '@/components/motion'

interface BottomSheetProps {
  children?: React.ReactNode
  defaultOpen?: boolean
  open?: boolean
  onClose?: () => void
  viewCloseBar?: boolean
}

export function BottomSheet({
  children,
  defaultOpen = false,
  open,
  onClose,
  viewCloseBar = true,
}: BottomSheetProps) {
  const [innerOpen, setInnerOpen] = useState(defaultOpen)
  const resultOpen = open ?? innerOpen

  return (
    <AnimatePresence>
      {resultOpen && (
        <MotionDiv
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className={css({
            bg: 'rgba(0, 0, 0, 0.5)',
            pos: 'fixed',
            inset: 0,
            zIndex: 100,
          })}
          onClick={() => {
            setInnerOpen(false)
            onClose?.()
          }}
        >
          <MotionDiv
            initial={{ y: '100%' }}
            animate={{ y: '0%' }}
            exit={{ y: '100%' }}
            className={css({
              bg: '$containerBackground',
              borderRadius: '20px 20px 0 0',
              pos: 'fixed',
              bottom: 0,
              w: '100%',
              maxW: [null, null, '420px'],
              mx: 'auto',
            })}
            onClick={(e) => e.stopPropagation()}
          >
            {viewCloseBar && (
              <Center h="30px" w="100%">
                <Box bg="$border" borderRadius="2px" h="5px" w="48px" />
              </Center>
            )}
            {children}
          </MotionDiv>
        </MotionDiv>
      )}
    </AnimatePresence>
  )
}
```

## 5. Loading Pattern

```tsx
// components/layout/loading/Loading.tsx
import { Box, Center, Flex, Text, VStack } from '@devup-ui/react'

interface LoadingProps {
  message?: React.ReactNode
}

export function Loading({ message }: LoadingProps) {
  return (
    <Center
      backdropFilter="blur(5px)"
      bg="#FFFFFF80"
      h="100vh"
      left="0"
      position="fixed"
      top="0"
      w="100%"
      zIndex="9999"
    >
      <VStack alignItems="center" gap="30px">
        <Flex gap="10px">
          {[1, 0.7, 0.4].map((opacity, i) => (
            <Box
              key={i}
              aspectRatio="1"
              bg="$primary"
              borderRadius="1000px"
              boxSize="12px"
              opacity={opacity}
            />
          ))}
        </Flex>
        <Text color="$text" textAlign="center" typography="bodySemibold">
          {message || '로딩 중...'}
        </Text>
      </VStack>
    </Center>
  )
}
```

## 6. Empty State Pattern

```tsx
import { Center, Text, VStack } from '@devup-ui/react'
import Image from 'next/image'

interface EmptyDataProps {
  icon?: string
  message: string
}

export function EmptyData({ icon = '/icons/emptyData.svg', message }: EmptyDataProps) {
  return (
    <Center py="60px">
      <VStack alignItems="center" gap="16px">
        <Image alt="empty" height={48} src={icon} width={48} />
        <Text color="$caption" typography="body" textAlign="center">
          {message}
        </Text>
      </VStack>
    </Center>
  )
}
```

## 7. Button Variant Pattern

```tsx
import { css } from '@devup-ui/react'
import clsx from 'clsx'

const buttonVariants = {
  primary: css({
    bg: '$primary',
    color: '#FFF',
    _hover: { bg: '$primaryBold' },
    _active: { bg: '$primaryExBold', scale: 0.95 },
  }),
  secondary: css({
    bg: '$containerBackground',
    color: '$text',
    border: '1px solid $border',
    _hover: { bg: '$gray100' },
  }),
  sub: css({
    bg: '$primaryBg',
    color: '$primary',
    _hover: { bg: '$primaryBgBold', color: '$primaryBold' },
  }),
  disabled: css({
    bg: '$gray200',
    color: '$gray400',
    cursor: 'not-allowed',
  }),
}

interface ButtonProps {
  variant: keyof typeof buttonVariants
  className?: string
  children: React.ReactNode
}

export function Button({ variant, className, children, ...props }: ButtonProps) {
  return (
    <Box
      as="button"
      className={clsx(buttonVariants[variant], className)}
      borderRadius="12px"
      px="20px"
      py="14px"
      styleOrder={1}
      transition="all 0.2s"
      {...props}
    >
      {children}
    </Box>
  )
}
```

## 8. Tab Pattern

```tsx
'use client'
import { Box, Flex, Text } from '@devup-ui/react'
import { useState } from 'react'

interface Tab {
  id: string
  label: string
}

interface TabsProps {
  tabs: Tab[]
  defaultTab?: string
  onChange?: (tabId: string) => void
}

export function Tabs({ tabs, defaultTab, onChange }: TabsProps) {
  const [activeTab, setActiveTab] = useState(defaultTab ?? tabs[0]?.id)

  const handleTabClick = (tabId: string) => {
    setActiveTab(tabId)
    onChange?.(tabId)
  }

  return (
    <Flex borderBottom="1px solid $border" gap="24px">
      {tabs.map((tab) => (
        <Box
          key={tab.id}
          borderBottom={activeTab === tab.id ? '2px solid $primary' : 'none'}
          cursor="pointer"
          mb="-1px"
          onClick={() => handleTabClick(tab.id)}
          pb="12px"
        >
          <Text
            color={activeTab === tab.id ? '$primary' : '$caption'}
            typography={activeTab === tab.id ? 'tabBold' : 'tabText'}
          >
            {tab.label}
          </Text>
        </Box>
      ))}
    </Flex>
  )
}
```

## 9. Responsive Layout Pattern

```tsx
// 모바일 퍼스트, 최대 너비 420px 제한
<Box
  maxW={['100%', null, '420px']}
  mx="auto"
  minH="100vh"
  bg="$background"
>
  {/* Content */}
</Box>

// 반응형 그리드
<Grid
  columns={['1fr', null, '1fr 1fr']}
  gap={['16px', null, '24px']}
>
  {/* Grid items */}
</Grid>

// 반응형 숨김/표시
<Box display={['none', null, 'block']}>
  데스크탑에서만 표시
</Box>
<Box display={['block', null, 'none']}>
  모바일에서만 표시
</Box>
```

## 10. Motion Pattern

```tsx
// components/motion.ts
'use client'
import { motion } from 'framer-motion'

export const MotionDiv = motion.div
export const MotionBox = motion.div

// 사용 예시
<MotionDiv
  initial={{ opacity: 0, y: 20 }}
  animate={{ opacity: 1, y: 0 }}
  exit={{ opacity: 0, y: -20 }}
  transition={{ duration: 0.2 }}
>
  {content}
</MotionDiv>
```

## 11. Status Tag Pattern

```tsx
import { Box, Text } from '@devup-ui/react'

type Status = 'success' | 'warning' | 'error' | 'info'

const statusColors: Record<Status, { bg: string; color: string }> = {
  success: { bg: '$greenTagBg', color: '$greenTag' },
  warning: { bg: '$orangeTagBg', color: '$orangeTag' },
  error: { bg: '$redTagBg', color: '$redTag' },
  info: { bg: '$blueTagBg', color: '$blueTag' },
}

interface StatusTagProps {
  status: Status
  label: string
}

export function StatusTag({ status, label }: StatusTagProps) {
  const { bg, color } = statusColors[status]
  
  return (
    <Box bg={bg} borderRadius="4px" px="8px" py="4px">
      <Text color={color} typography="tag">
        {label}
      </Text>
    </Box>
  )
}
```

## 12. Card Pattern

```tsx
import { Box, Text, VStack } from '@devup-ui/react'

interface CardProps {
  title: string
  description?: string
  children?: React.ReactNode
  onClick?: () => void
}

export function Card({ title, description, children, onClick }: CardProps) {
  return (
    <Box
      bg="$containerBackground"
      border="1px solid $border"
      borderRadius="12px"
      cursor={onClick ? 'pointer' : 'default'}
      onClick={onClick}
      p="16px"
      transition="all 0.2s"
      _hover={onClick ? { boxShadow: '0 4px 12px rgba(0,0,0,0.1)' } : undefined}
    >
      <VStack alignItems="flex-start" gap="8px">
        <Text color="$title" typography="bodyTitle">{title}</Text>
        {description && (
          <Text color="$text" typography="body">{description}</Text>
        )}
        {children}
      </VStack>
    </Box>
  )
}
```
