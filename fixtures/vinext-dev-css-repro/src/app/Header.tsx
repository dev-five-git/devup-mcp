'use client'
import { Box, Flex, Text } from '@devup-ui/react'
import { useState } from 'react'
export default function Header() {
  const [open, setOpen] = useState(false)
  return <Flex as="header" w="100%" p="16px" justifyContent="space-between">
    <Text color="$text">Shared client header</Text>
    <Box as="button" bg="#eee" onClick={() => setOpen(!open)}>{open ? 'Close' : 'Menu'}</Box>
  </Flex>
}
