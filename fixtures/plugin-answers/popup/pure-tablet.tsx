<VStack bg="#000000B2" overflow="hidden" px="184px" py="279.5px">
    <VStack borderRadius="12px" boxShadow="0 0 20px 0 #00000040" overflow="hidden">
        <VStack
            alignItems="center"
            bg="$cardBg"
            gap="40px"
            overflow="hidden"
            p="40px"
        >
            <VStack gap="30px">
                <Flex alignItems="center" gap="16px">
                    <Image aspectRatio="1" boxSize="24px" src="/icons/megaphone_760116 1.svg" />
                    <Text color="$primary" typography="textboxTitle" wordBreak="keep-all">
                        안내
                    </Text>
                </Flex>
                <VStack gap="16px">
                    <Text color="$text" typography="modalText" wordBreak="keep-all">
                        어려운 글을 쉽게 바꿔 주는 서비스 {"'"}온글{"'"}은 <br />2026년 하반기부터 정식 서비스로 찾아올 <br />예정입니다.{" "}
                    </Text>
                    <Text color="$text" typography="modalText" wordBreak="keep-all">
                        온글의 소식을 가장 먼저 받아보고 싶다면 <br />아래 버튼을 눌러 주세요.
                    </Text>
                </VStack>
            </VStack>
            <Center bg="$primary" borderRadius="1000px" px="40px" py="16px">
                <Text color="#FFF" typography="buttonSm" wordBreak="keep-all">
                    서비스 소식 알림 받기
                </Text>
            </Center>
        </VStack>
        <Flex alignItems="center" bg="$containerBackground" justifyContent="space-between" overflow="hidden">
            <Flex alignItems="center" p="20px">
                <Text color="$caption" typography="modalBtn" wordBreak="keep-all">
                    오늘 하루 그만 보기
                </Text>
            </Flex>
            <Flex alignItems="center" gap="10px" p="20px">
                <Box
                    aspectRatio="1"
                    bg="$caption"
                    boxSize="16px"
                    maskImage="url('/icons/Frame 1000013807.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                    transform="rotate(-90deg)"
                />
                <Text color="$caption" typography="modalBtn" wordBreak="keep-all">
                    닫기
                </Text>
            </Flex>
        </Flex>
    </VStack>
</VStack>
