import { Box, Center, Text, VStack } from "@devup-ui/react";

export function _4단계() {
  return (
    <VStack bg="$containerBackground" h="740px" overflow="hidden">
      <VStack
        bg="$containerBackground"
        flex="1"
        gap="24px"
        px="20px"
        py="30px"
      >
        <Box
          aspectRatio="1"
          bg="$primary"
          borderRadius="12px"
          boxSize="48px"
          maskImage="url(/icons/Favicon.svg)"
          maskPos="center"
          maskRepeat="no-repeat"
          maskSize="contain"
        />
        <VStack gap="6px">
          <Text color="$title" typography="h4" wordBreak="keep-all">
            이야기가 전달되었어요.
          </Text>
          <Text color="$textLight" typography="caption" wordBreak="keep-all">
            이야기가 성공적으로 전달되었어요! <br />질문을 보낸 분이 확인할 수 있도록 <br />저희가 알림을 보내드릴게요.
          </Text>
        </VStack>
        <Box bg="$border" h="1px" />
        <Center flexDir="column" gap="12px">
          <Text color="$primary" typography="bodyTitle" w="100%" wordBreak="keep-all">
            내 어린 시절 가장 기억나는 순간은?
          </Text>
          <Text color="$text" typography="bodySm" w="100%" wordBreak="keep-all">
            길동이가 다섯 살 때, 처음으로 혼자 자전거를 탔던 날이 가장 기억에 남아. 몇 번을 넘어져도 다시 일어나던 모습이 참 대견했단다.
          </Text>
        </Center>
      </VStack>
      <VStack px="10px" py="8px">
        <Center
          bg="$primary"
          border="solid 1px $primary"
          borderRadius="12px"
          px="19px"
          py="13px"
          w="340px"
        >
          <Text color="#FFF" typography="bodySemibold" wordBreak="keep-all">
            기록공간이 궁금해요
          </Text>
        </Center>
      </VStack>
    </VStack>
  );
}
