import { Text, VStack } from '@devup-ui/react'
export default function About() {
  return <VStack w="100%" p={['20px', null, null, '40px']}>
    <Text fontSize={['36px', null, null, '48px']}>About</Text>
    <a href="/seo-geo/">SEO</a>
  </VStack>
}
