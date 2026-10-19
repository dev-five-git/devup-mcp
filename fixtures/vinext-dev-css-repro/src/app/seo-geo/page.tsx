import { Center, Flex, Text, VStack } from '@devup-ui/react'
import Form from './Form'
export default function Seo() {
  return <VStack w="100%" bg="$background">
    <Center pt={['104px', null, null, '128px']} px={['20px', null, null, '40px']}>
      <VStack w="100%" maxW="980px" gap="24px">
        <Text fontSize={['36px', null, null, '48px']}>SEO diagnosis</Text>
        <Form />
        <Flex flexDir={['column', null, null, 'row']} gap="20px">
          <Text flex="1">Search engine optimization content</Text>
          <Text flex="1">Generative engine optimization content</Text>
        </Flex>
      </VStack>
    </Center>
  </VStack>
}
