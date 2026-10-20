'use client'
import { Box, Text, VStack } from '@devup-ui/react'
import { useState } from 'react'
export default function Form() {
  const [value, setValue] = useState('')
  return <VStack gap="17px" p="29px" border="1px solid #ddd">
    <Text fontSize="19px">Diagnosis</Text>
    <Box as="input" value={value} onChange={(event) => setValue(event.target.value)} p="13px" />
  </VStack>
}
