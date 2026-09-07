<Center bg="$background" flexDir="column" overflow="hidden">
    <VStack
        alignItems="center"
        bg="linear-gradient(-180deg, #E1E5F5 0%, #FEF4FF 100%)"
        overflow="hidden"
        pb="100px"
        pt="200px"
        w="100%"
    >
        <VStack
            gap="40px"
            justifyContent="center"
            maxW="1440px"
            pos="relative"
            px="40px"
            w="100%"
        >
            <Image
                h="100%"
                left="609px"
                pos="absolute"
                src="/images/418625428_72f26dbd-47e8-4138-9fb0-a2e7a8fa07ff 1.png"
                top="-329px"
                w="1232px"
            />
            <VStack gap="24px" justifyContent="center">
                <Text color="$title" typography="h1">
                    <Text color="$primary">
                        Zero
                    </Text>
                    {" "}Config{" "}
                    <Text color="$primary">
                        Zero
                    </Text>
                    {" "}FOUC{" "}
                    <Text color="$primary">
                        Zero
                    </Text>
                    {" "}Runtime<br />CSS in JS Preprocessor
                </Text>
                <Text color="$text" typography="h6Reg">
                    Building the Future of CSS-in-JS Analyze all possible scenarios at the fastest speed and style with optimal performance.
                </Text>
            </VStack>
            <Flex
                alignItems="center"
                bg="$text"
                borderRadius="100px"
                gap="20px"
                px="40px"
                py="16px"
            >
                <Box bg="$secondary" borderRadius="50%" boxSize="10px" />
                <Flex alignItems="center" gap="10px">
                    <Text color="$base" typography="buttonL">
                        Get started
                    </Text>
                    <Box
                        bg="$base"
                        boxSize="24px"
                        maskImage="url(/icons/icons.svg)"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                    />
                </Flex>
            </Flex>
            <Flex gap="12px">
                <Flex alignItems="center" bg="$containerBackground" border="solid 1px $imageBorder" borderRadius="12px">
                    <Flex
                        alignItems="center"
                        borderRadius="12px 0 0 12px"
                        gap="10px"
                        pl="16px"
                        pr="20px"
                        py="10px"
                    >
                        <Image aspectRatio="1" boxSize="24px" src="/icons/solar:star-bold.svg" />
                        <Text color="$text" typography="buttonLsemiB">
                            Star
                        </Text>
                    </Flex>
                    <Center
                        bg="$starBg"
                        border="solid 1px $imageBorder"
                        borderRadius="0 12px 12px 0"
                        flexDir="column"
                        h="100%"
                        px="16px"
                    >
                        <Text color="$primary" textAlign="center" typography="buttonLsemiB" w="100%">
                            36
                        </Text>
                    </Center>
                </Flex>
                <Flex
                    alignItems="center"
                    bg="$containerBackground"
                    border="solid 1px $imageBorder"
                    borderRadius="12px"
                    gap="10px"
                    pl="16px"
                    pr="20px"
                    py="10px"
                >
                    <Image aspectRatio="1" boxSize="24px" src="/icons/solar:heart-bold.svg" />
                    <Text color="$text" typography="buttonLsemiB">
                        Sponsor
                    </Text>
                </Flex>
            </Flex>
        </VStack>
    </VStack>
    <VStack alignItems="center" py="60px" w="100%">
        <VStack gap="30px" maxW="1440px" px="40px" w="100%">
            <VStack gap="16px" w="805px">
                <Text color="$title" typography="h4">
                    Comparison Bechmarks
                </Text>
                <Text color="$text" typography="textL">
                    Next.js Build Time and Build Size (github action - ubuntu-latest)
                </Text>
            </VStack>
            <Flex alignItems="center" gap="20px">
                <VStack
                    bg="$containerBackground"
                    border="solid 1px $border"
                    borderRadius="20px"
                    boxShadow="0 0 8px 0 $shadow"
                    gap="60px"
                    justifyContent="center"
                    overflow="hidden"
                    p="30px"
                    pos="relative"
                    w="400px"
                >
                    <Image
                        left="-60px"
                        opacity="0.2"
                        pos="absolute"
                        src="/icons/Group 1.svg"
                        top="132px"
                        w="260px"
                    />
                    <Image
                        bottom="-30px"
                        opacity="0.2"
                        pos="absolute"
                        right="200px"
                        src="/icons/Group 2.svg"
                        w="260px"
                    />
                    <VStack gap="8px">
                        <Text color="$text" typography="h5">
                            Devup-ui
                        </Text>
                        <Text color="$text" typography="textL">
                            1.0.15
                        </Text>
                    </VStack>
                    <VStack alignItems="flex-end" gap="20px">
                        <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                            <Text color="$text" typography="textSbold">
                                Next.js Build TIme{" "}
                            </Text>
                            <Flex gap="10px">
                                <Box
                                    aspectRatio="1"
                                    bg="#FFC100"
                                    boxSize="24px"
                                    maskImage="url('/icons/crown_17099645 1.svg')"
                                    maskPos="center"
                                    maskRepeat="no-repeat"
                                    maskSize="contain"
                                />
                                <Text WebkitTextFillColor="transparent" bg="linear-gradient(-90deg, #6BB1F2 0%, #8235CA 100%)" bgClip="text" typography="h4">
                                    16.8s
                                </Text>
                            </Flex>
                        </VStack>
                        <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                            <Text color="$text" typography="textSbold">
                                Bulid Size
                            </Text>
                            <Flex gap="10px">
                                <Box
                                    aspectRatio="1"
                                    bg="#FFC100"
                                    boxSize="24px"
                                    maskImage="url('/icons/crown_17099645 1.svg')"
                                    maskPos="center"
                                    maskRepeat="no-repeat"
                                    maskSize="contain"
                                />
                                <Text WebkitTextFillColor="transparent" bg="linear-gradient(-90deg, #6BB1F2 0%, #8235CA 100%)" bgClip="text" typography="h4">
                                    51.2MB
                                </Text>
                            </Flex>
                        </VStack>
                    </VStack>
                </VStack>
                <VStack
                    bg="$cardBg"
                    borderRadius="20px"
                    flex="1"
                    gap="40px"
                    justifyContent="center"
                    p="30px"
                >
                    <VStack gap="8px">
                        <Text color="$captionBold" typography="h6">
                            Chakra UI
                        </Text>
                        <Text color="$captionBold" typography="textL">
                            3.24.2
                        </Text>
                    </VStack>
                    <VStack alignItems="flex-end" gap="20px">
                        <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                            <Text color="$captionBold" typography="textSbold">
                                Bulid Time
                            </Text>
                            <Text color="$caption" typography="h5">
                                29.3s
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                            <Text color="$captionBold" typography="textSbold">
                                Bulid Size
                            </Text>
                            <Text color="$caption" typography="h5">
                                186.2MB
                            </Text>
                        </VStack>
                    </VStack>
                </VStack>
                <VStack
                    bg="$cardBg"
                    borderRadius="20px"
                    flex="1"
                    gap="40px"
                    justifyContent="center"
                    p="30px"
                >
                    <VStack gap="8px">
                        <Text color="$captionBold" typography="h6">
                            Mui
                        </Text>
                        <Text color="$captionBold" typography="textL">
                            7.3.1
                        </Text>
                    </VStack>
                    <VStack alignItems="flex-end" gap="20px">
                        <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                            <Text color="$captionBold" typography="textSbold">
                                Bulid Time
                            </Text>
                            <Text color="$caption" typography="h5">
                                21.6s
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                            <Text color="$captionBold" typography="textSbold">
                                Bulid Size
                            </Text>
                            <Text color="$caption" typography="h5">
                                84.3MB
                            </Text>
                        </VStack>
                    </VStack>
                </VStack>
                <VStack
                    bg="$cardBg"
                    borderRadius="20px"
                    flex="1"
                    gap="40px"
                    justifyContent="center"
                    p="30px"
                >
                    <VStack gap="8px">
                        <Text color="$captionBold" typography="h6">
                            Kuma UI
                        </Text>
                        <Text color="$captionBold" typography="textL">
                            1.5.9
                        </Text>
                    </VStack>
                    <VStack alignItems="flex-end" gap="20px">
                        <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                            <Text color="$captionBold" typography="textSbold">
                                Bulid Time
                            </Text>
                            <Text color="$caption" typography="h5">
                                20.6s
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                            <Text color="$captionBold" typography="textSbold">
                                Bulid Size
                            </Text>
                            <Text color="$caption" typography="h5">
                                60.3B
                            </Text>
                        </VStack>
                    </VStack>
                </VStack>
            </Flex>
        </VStack>
    </VStack>
    <VStack alignItems="center" overflow="hidden" py="50px" w="100%">
        <VStack gap="40px" maxW="1440px" px="40px" w="100%">
            <VStack gap="16px">
                <Text color="$title" typography="h4">
                    Features
                </Text>
                <Text color="$text" typography="textL">
                    Devup UI offers a performance-optimized CSS-in-JS system, theme typing, <br />and amazing features for faster and safer development.
                </Text>
            </VStack>
            <VStack gap="16px">
                <Flex alignItems="center" gap="16px">
                    <VStack
                        bg="$containerBackground"
                        border="solid 1px $border"
                        borderRadius="20px"
                        boxShadow="0 4px 12px 0 #8787870F"
                        flex="1"
                        gap="20px"
                        h="100%"
                        overflow="hidden"
                        p="30px"
                    >
                        <Image boxSize="32px" src="/icons/010.idea.svg" />
                        <VStack gap="10px">
                            <Text color="$title" typography="h6">
                                Zero Runtime
                            </Text>
                            <Text color="$text" typography="body">
                                A futuristic design that eliminates the root causes of performance degradation.
                            </Text>
                        </VStack>
                    </VStack>
                    <VStack
                        bg="$containerBackground"
                        border="solid 1px $border"
                        borderRadius="20px"
                        boxShadow="0 4px 12px 0 #8787870F"
                        flex="1"
                        gap="20px"
                        overflow="hidden"
                        p="30px"
                    >
                        <Image boxSize="32px" src="/icons/019.trophy.svg" />
                        <VStack gap="10px">
                            <Text color="$title" typography="h6">
                                Top Performance
                            </Text>
                            <Text color="$text" typography="body">
                                The fastest build speed and the smallest bundle size among CSS-in-JS solutions.
                            </Text>
                        </VStack>
                    </VStack>
                </Flex>
                <Flex alignItems="center" gap="16px">
                    <VStack
                        bg="$containerBackground"
                        border="solid 1px $border"
                        borderRadius="20px"
                        boxShadow="0 4px 12px 0 #8787870F"
                        flex="1"
                        gap="20px"
                        h="100%"
                        overflow="hidden"
                        p="30px"
                    >
                        <Image boxSize="32px" src="/icons/021.heart.svg" />
                        <VStack gap="10px">
                            <Text color="$title" typography="h6">
                                Type Safety
                            </Text>
                            <Text color="$text" typography="body">
                                Enhanced DX with typing-based support.
                            </Text>
                        </VStack>
                    </VStack>
                    <VStack
                        bg="$containerBackground"
                        border="solid 1px $border"
                        borderRadius="20px"
                        boxShadow="0 4px 12px 0 #8787870F"
                        flex="1"
                        gap="20px"
                        h="100%"
                        overflow="hidden"
                        p="30px"
                        pos="relative"
                    >
                        <Image boxSize="32px" src="/icons/016.notice.svg" />
                        <VStack gap="10px">
                            <Text color="$title" typography="h6">
                                Figma Plugin
                            </Text>
                            <Text color="$text" typography="body">
                                A Figma plugin enabling safer and faster development.{" "}
                            </Text>
                        </VStack>
                        <VStack
                            borderRadius="8px"
                            p="8px"
                            pos="absolute"
                            right="20px"
                            top="20px"
                        >
                            <Flex alignItems="center" gap="4px" justifyContent="flex-end">
                                <Text color="$primary" typography="body">
                                    Go Figma Community
                                </Text>
                                <Box
                                    aspectRatio="1"
                                    bg="$primary"
                                    boxSize="16px"
                                    maskImage="url(/icons/icons.svg)"
                                    maskPos="center"
                                    maskRepeat="no-repeat"
                                    maskSize="contain"
                                />
                            </Flex>
                        </VStack>
                    </VStack>
                </Flex>
            </VStack>
        </VStack>
    </VStack>
    <VStack
        alignItems="center"
        pb="100px"
        pt="40px"
        px="30px"
        w="100%"
    >
        <Flex
            alignItems="center"
            bg="$joinBg"
            borderRadius="40px 40px 0"
            justifyContent="flex-end"
            maxW="1440px"
            overflow="hidden"
            p="80px"
            pos="relative"
            w="100%"
        >
            <Image
                left="-277px"
                pos="absolute"
                src="/icons/Group 2.svg"
                top="-187px"
                w="1102px"
            />
            <VStack alignItems="flex-end" gap="50px" justifyContent="center" w="547px">
                <VStack alignItems="flex-end" gap="16px" justifyContent="center">
                    <Text color="$text" typography="h4">
                        Join our community
                    </Text>
                    <Text color="$text" typography="textL">
                        Join our Discord and help build the future of frontend with CSS-in-JS!{" "}
                    </Text>
                </VStack>
                <Flex gap="10px">
                    <Flex
                        alignItems="center"
                        bg="$kakaoButton"
                        borderRadius="100px"
                        px="40px"
                        py="16px"
                    >
                        <Flex alignItems="center" gap="10px">
                            <Text color="#FFF" typography="buttonLbold">
                                Open KakaoTalk
                            </Text>
                            <Box
                                bg="#FFF"
                                boxSize="24px"
                                maskImage="url(/icons/icons.svg)"
                                maskPos="center"
                                maskRepeat="no-repeat"
                                maskSize="contain"
                            />
                        </Flex>
                    </Flex>
                    <Flex
                        alignItems="center"
                        bg="$buttonBlue"
                        borderRadius="100px"
                        px="40px"
                        py="16px"
                    >
                        <Flex alignItems="center" gap="10px">
                            <Text color="#FFF" typography="buttonLbold">
                                Join our Discord
                            </Text>
                            <Box
                                bg="#FFF"
                                boxSize="24px"
                                maskImage="url(/icons/icons.svg)"
                                maskPos="center"
                                maskRepeat="no-repeat"
                                maskSize="contain"
                            />
                        </Flex>
                    </Flex>
                </Flex>
            </VStack>
        </Flex>
    </VStack>
    <Center
        bg="$footerBg"
        flexDir="column"
        px="30px"
        py="60px"
        w="1920px"
    >
        <Flex justifyContent="space-between" maxW="1440px" w="100%">
            <Image src="/images/Frame 1000014051.png" />
            <VStack alignItems="flex-end" gap="10px" justifyContent="center">
                <Text color="$footerText" textAlign="right" typography="small" wordBreak="keep-all">
                    상호: (주)데브파이브 | 대표자명: 오정민 | 사업자등록번호: 868-86-03159<br />주소: 경기 고양시 덕양구 마상로140번길 81 4층
                </Text>
                <Text color="$footerTitle" typography="small" wordBreak="keep-all">
                    Copyright © 2021-2024 데브파이브. All Rights Reserved.{" "}
                </Text>
            </VStack>
        </Flex>
    </Center>
    <VStack
        alignItems="center"
        cursor="pointer"
        left="0px"
        pos="absolute"
        pt="20px"
        px="30px"
        top="0px"
        w="100%"
    >
        <Flex
            alignItems="center"
            bg="$containerBackground"
            borderRadius="16px"
            boxShadow="0 2px 8px 0 $shadow"
            h="70px"
            justifyContent="space-between"
            maxW="1440px"
            overflow="hidden"
            px="20px"
            w="100%"
        >
            <Image aspectRatio="4.16" h="42px" src="/icons/Group 6.svg" w="175px" />
            <Flex alignItems="center" gap="10px" h="100%">
                <Center h="100%" px="24px">
                    <Text color="$title" opacity="0.6" typography="buttonLsemiB">
                        Docs
                    </Text>
                </Center>
                <Center h="100%" px="24px">
                    <Text color="$title" opacity="0.6" typography="buttonLsemiB">
                        Components
                    </Text>
                </Center>
                <Center cursor="pointer" h="100%" px="24px">
                    <Text color="$title" opacity="0.6" typography="buttonLsemiB">
                        Showcase
                    </Text>
                </Center>
                <Center h="100%" px="24px">
                    <Text color="$title" opacity="0.6" typography="buttonLsemiB">
                        Team
                    </Text>
                </Center>
                <Center gap="4px" h="100%" px="24px">
                    <Text color="$title" opacity="0.6" typography="buttonLsemiB">
                        Stroybook
                    </Text>
                    <Box
                        aspectRatio="1"
                        bg="$captionBold"
                        boxSize="20px"
                        maskImage="url(/icons/icons.svg)"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                        opacity="0.6"
                    />
                </Center>
                <Flex alignItems="center" h="100%">
                    <Box
                        bg="$title"
                        boxSize="24px"
                        h="70px"
                        maskImage="url('/icons/gnb icon.svg')"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                        w="44px"
                    />
                    <Box
                        bg="$title"
                        boxSize="24px"
                        h="70px"
                        maskImage="url('/icons/gnb icon.svg')"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                        w="44px"
                    />
                    <Box
                        bg="$title"
                        boxSize="24px"
                        h="70px"
                        maskImage="url('/icons/gnb icon.svg')"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                        w="44px"
                    />
                    <Box
                        bg="$title"
                        boxSize="24px"
                        cursor="pointer"
                        h="70px"
                        maskImage="url('/icons/gnb icon.svg')"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                        w="44px"
                    />
                </Flex>
            </Flex>
        </Flex>
    </VStack>
</Center>
