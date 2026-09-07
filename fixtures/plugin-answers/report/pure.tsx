<Center
    bg="linear-gradient(90deg, color-mix(in srgb, $primaryBg, transparent 50%) 0%, color-mix(in srgb, $secondaryBg, transparent 50%) 100%), $containerBackground"
    flexDir="column"
    overflow="hidden"
    px="30px"
    py="80px"
>
    <VStack
        alignItems="flex-end"
        gap="40px"
        maxW="1280px"
        pos="relative"
        w="100%"
    >
        <Image
            bottom="-284.8px"
            left="-81px"
            pos="absolute"
            src="/icons/Group 20.svg"
            transform="rotate(-4.01deg)"
            transformOrigin="top left"
            w="686.26px"
        />
        <VStack alignItems="flex-end" gap="12px" justifyContent="center">
            <Text
                WebkitTextFillColor="transparent"
                bg="linear-gradient(90deg, $primary 0%, $secondary 100%)"
                bgClip="text"
                typography="h4"
                wordBreak="keep-all"
            >
                상세한 분석 리포트
            </Text>
            <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                과학적 데이터를 기반으로 한 맞춤형 분석 결과를 제공합니다
            </Text>
        </VStack>
        <VStack gap="24px" justifyContent="center" maxW="540px" w="100%">
            <VStack
                backdropFilter="blur(10px)"
                bg="linear-gradient(90deg, color-mix(in srgb, $containerBackground, transparent 20%) 0%, color-mix(in srgb, $containerBackground50per, transparent 60%) 100%)"
                borderRadius="16px"
                boxShadow="0 4px 6px -4px #0000001A, 0 10px 15px -3px #7D82951A"
                gap="12px"
                overflow="hidden"
                p="24px"
            >
                <Flex alignItems="center" gap="16px">
                    <Center bg="$primaryBg" borderRadius="12px" boxSize="48px">
                        <Box
                            aspectRatio="1"
                            bg="$primary"
                            boxSize="24px"
                            maskImage="url(/icons/Icons.svg)"
                            maskPos="center"
                            maskRepeat="no-repeat"
                            maskSize="contain"
                        />
                    </Center>
                    <Text color="$title" typography="titleSm" wordBreak="keep-all">
                        ADHD 성향 분석
                    </Text>
                </Flex>
                <Text color="$textLight" typography="body" wordBreak="keep-all">
                    주의력, 충동성, 과잉행동 등 주요 영역별 점수를 시각화하여 제공합니다.
                </Text>
            </VStack>
            <VStack
                backdropFilter="blur(10px)"
                bg="linear-gradient(90deg, color-mix(in srgb, $containerBackground, transparent 20%) 0%, color-mix(in srgb, $containerBackground50per, transparent 60%) 100%)"
                borderRadius="16px"
                boxShadow="0 4px 6px -4px #0000001A, 0 10px 15px -3px #7D82951A"
                gap="12px"
                overflow="hidden"
                p="24px"
            >
                <Flex alignItems="center" gap="16px">
                    <Center bg="$primaryBg" borderRadius="12px" boxSize="48px">
                        <Box
                            aspectRatio="1"
                            bg="$primary"
                            boxSize="24px"
                            maskImage="url(/icons/Icons.svg)"
                            maskPos="center"
                            maskRepeat="no-repeat"
                            maskSize="contain"
                        />
                    </Center>
                    <Text color="$title" typography="titleSm" wordBreak="keep-all">
                        일상생활 패턴
                    </Text>
                </Flex>
                <Text color="$textLight" typography="body" wordBreak="keep-all">
                    업무, 학업, 대인관계 등 일상 영역별 영향도를 분석합니다.
                </Text>
            </VStack>
            <VStack
                backdropFilter="blur(10px)"
                bg="linear-gradient(90deg, color-mix(in srgb, $containerBackground, transparent 20%) 0%, color-mix(in srgb, $containerBackground50per, transparent 60%) 100%)"
                borderRadius="16px"
                boxShadow="0 4px 6px -4px #0000001A, 0 10px 15px -3px #7D82951A"
                gap="12px"
                overflow="hidden"
                p="24px"
            >
                <Flex alignItems="center" gap="16px">
                    <Center bg="$primaryBg" borderRadius="12px" boxSize="48px">
                        <Box
                            aspectRatio="1"
                            bg="$primary"
                            boxSize="24px"
                            maskImage="url(/icons/Icons.svg)"
                            maskPos="center"
                            maskRepeat="no-repeat"
                            maskSize="contain"
                        />
                    </Center>
                    <Text color="$title" typography="titleSm" wordBreak="keep-all">
                        맞춤형 전략 제안
                    </Text>
                </Flex>
                <Text color="$textLight" typography="body" wordBreak="keep-all">
                    개인의 특성을 고려한 구체적인 대처 전략을 제시합니다.
                </Text>
            </VStack>
        </VStack>
    </VStack>
</Center>
