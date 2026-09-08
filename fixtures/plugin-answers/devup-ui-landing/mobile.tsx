<Center bg="$background" flexDir="column" overflow="hidden">
    <Gnb />
    <VStack
        alignItems="center"
        bg="linear-gradient(-180deg, #E1E5F5 0%, #FEF4FF 100%)"
        overflow="hidden"
        pb="240px"
        pt="40px"
        w="100%"
    >
        <Center
            flexDir="column"
            gap="40px"
            maxW="1440px"
            pos="relative"
            px="16px"
            w="100%"
        >
            <Image
                boxSize="100%"
                opacity="0.8"
                pos="absolute"
                right="-74px"
                src="/images/418625428_72f26dbd-47e8-4138-9fb0-a2e7a8fa07ff 1.png"
                top="395px"
            />
            <VStack gap="24px" justifyContent="center" w="100%">
                <Text color="$title" textAlign="center" typography="h1">
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
                <Text color="$text" textAlign="center" typography="h6Reg">
                    Building the Future of CSS-in-JS Analyze all possible scenarios at the fastest speed and style with optimal performance.
                </Text>
            </VStack>
            <Button Property1="Default" />
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
        </Center>
    </VStack>
    <VStack alignItems="center" py="40px" w="100%">
        <VStack gap="30px" maxW="1440px" px="16px" w="100%">
            <VStack alignItems="center" gap="16px">
                <Text color="$title" typography="h4">
                    Comparison Bechmarks
                </Text>
                <Text color="$text" textAlign="center" typography="textL" w="100%">
                    Next.js Build Time and Build Size <br />(github action - ubuntu-latest)
                </Text>
            </VStack>
            <VStack gap="20px" justifyContent="center">
                <Flex
                    bg="$containerBackground"
                    border="solid 1px $border"
                    borderRadius="20px"
                    boxShadow="0 0 8px 0 $shadow"
                    gap="60px"
                    overflow="hidden"
                    p="24px"
                    pos="relative"
                >
                    <Image
                        left="-60px"
                        opacity="0.2"
                        pos="absolute"
                        src="/icons/Group 1.svg"
                        top="-42px"
                        w="260px"
                    />
                    <VStack gap="8px">
                        <Text color="$text" typography="h5">
                            Devup-ui
                        </Text>
                        <Text color="$text" typography="textL">
                            1.0.18
                        </Text>
                    </VStack>
                    <VStack alignItems="flex-end" flex="1" gap="20px">
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
                                    18.2s
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
                                    54.7MB
                                </Text>
                            </Flex>
                        </VStack>
                    </VStack>
                </Flex>
                <Flex gap="12px">
                    <Flex
                        bg="$cardBg"
                        borderRadius="20px"
                        gap="20px"
                        p="24px"
                        w="240px"
                    >
                        <VStack gap="8px">
                            <Text color="$captionBold" typography="h6">
                                Chakra UI
                            </Text>
                            <Text color="$captionBold" typography="textL">
                                3.27.0
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" flex="1" gap="20px">
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Time
                                </Text>
                                <Text color="$caption" typography="h5">
                                    29.9s
                                </Text>
                            </VStack>
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Size
                                </Text>
                                <Text color="$caption" typography="h5">
                                    200.4MB
                                </Text>
                            </VStack>
                        </VStack>
                    </Flex>
                    <Flex
                        bg="$cardBg"
                        borderRadius="20px"
                        gap="20px"
                        p="24px"
                        w="240px"
                    >
                        <VStack gap="8px">
                            <Text color="$captionBold" typography="h6">
                                Mui
                            </Text>
                            <Text color="$captionBold" typography="textL">
                                7.3.2
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" flex="1" gap="20px">
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
                    </Flex>
                    <Flex
                        bg="$cardBg"
                        borderRadius="20px"
                        gap="20px"
                        p="24px"
                        w="240px"
                    >
                        <VStack gap="8px">
                            <Text color="$captionBold" typography="h6">
                                Kuma UI
                            </Text>
                            <Text color="$captionBold" typography="textL">
                                1.5.9
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" flex="1" gap="20px">
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
                    </Flex>
                    <Flex
                        bg="$cardBg"
                        borderRadius="20px"
                        gap="20px"
                        p="24px"
                        w="240px"
                    >
                        <VStack gap="8px">
                            <Text color="$captionBold" typography="h6">
                                Kuma UI
                            </Text>
                            <Text color="$captionBold" typography="textL">
                                1.5.9
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" flex="1" gap="20px">
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
                    </Flex>
                    <Flex
                        bg="$cardBg"
                        borderRadius="20px"
                        gap="20px"
                        p="24px"
                        w="240px"
                    >
                        <VStack gap="8px">
                            <Text color="$captionBold" typography="h6">
                                Kuma UI
                            </Text>
                            <Text color="$captionBold" typography="textL">
                                1.5.9
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" flex="1" gap="20px">
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
                    </Flex>
                    <Flex
                        bg="$cardBg"
                        borderRadius="20px"
                        gap="20px"
                        p="24px"
                        w="240px"
                    >
                        <VStack gap="8px">
                            <Text color="$captionBold" typography="h6">
                                Kuma UI
                            </Text>
                            <Text color="$captionBold" typography="textL">
                                1.5.9
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" flex="1" gap="20px">
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
                    </Flex>
                    <Flex
                        bg="$cardBg"
                        borderRadius="20px"
                        gap="20px"
                        p="24px"
                        w="240px"
                    >
                        <VStack gap="8px">
                            <Text color="$captionBold" typography="h6">
                                Kuma UI
                            </Text>
                            <Text color="$captionBold" typography="textL">
                                1.5.9
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" flex="1" gap="20px">
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
                    </Flex>
                </Flex>
            </VStack>
        </VStack>
    </VStack>
    <VStack alignItems="center" overflow="hidden" py="30px" w="100%">
        <VStack gap="30px" maxW="1440px" px="16px" w="100%">
            <VStack alignItems="center" gap="16px">
                <Text color="$title" typography="h4">
                    Features
                </Text>
                <Text color="$text" textAlign="center" typography="textL" w="100%">
                    Devup UI offers a performance-optimized CSS-in-JS system, theme typing, <br />and amazing features for faster and safer development.
                </Text>
            </VStack>
            <VStack gap="16px">
                <Flex
                    bg="$containerBackground"
                    border="solid 1px $border"
                    borderRadius="12px"
                    boxShadow="0 4px 12px 0 #8787870F"
                    gap="20px"
                    overflow="hidden"
                    p="20px"
                >
                    {/* <010.idea style="flat" /> */}
                    <Image boxSize="32px" src="/icons/style=flat.svg" />
                    <VStack flex="1" gap="10px">
                        <Text color="$title" typography="h6">
                            Zero Runtime
                        </Text>
                        <Text color="$text" typography="body">
                            A futuristic design that eliminates the root causes of performance degradation.
                        </Text>
                    </VStack>
                </Flex>
                <Flex
                    bg="$containerBackground"
                    border="solid 1px $border"
                    borderRadius="12px"
                    boxShadow="0 4px 12px 0 #8787870F"
                    gap="20px"
                    overflow="hidden"
                    p="20px"
                >
                    {/* <019.trophy style="flat" /> */}
                    <Box
                        bg="#FFC738"
                        boxSize="32px"
                        maskImage="url(/icons/style=flat.svg)"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                    />
                    <VStack flex="1" gap="10px">
                        <Text color="$title" typography="h6">
                            Top Performance
                        </Text>
                        <Text color="$text" typography="body">
                            The fastest build speed and the smallest bundle size among CSS-in-JS solutions.
                        </Text>
                    </VStack>
                </Flex>
                <Flex
                    bg="$containerBackground"
                    border="solid 1px $border"
                    borderRadius="12px"
                    boxShadow="0 4px 12px 0 #8787870F"
                    gap="20px"
                    overflow="hidden"
                    p="20px"
                >
                    {/* <021.heart style="flat" /> */}
                    <Box
                        bg="#F4868F"
                        boxSize="32px"
                        maskImage="url(/icons/style=flat.svg)"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                    />
                    <VStack flex="1" gap="10px">
                        <Text color="$title" typography="h6">
                            Type Safety
                        </Text>
                        <Text color="$text" typography="body">
                            Enhanced DX with typing-based support.
                        </Text>
                    </VStack>
                </Flex>
                <Flex
                    bg="$containerBackground"
                    border="solid 1px $border"
                    borderRadius="12px"
                    boxShadow="0 4px 12px 0 #8787870F"
                    gap="20px"
                    overflow="hidden"
                    p="20px"
                    pos="relative"
                >
                    {/* <016.notice style="flat" /> */}
                    <Image boxSize="32px" src="/icons/style=flat.svg" />
                    <VStack flex="1" gap="10px">
                        <Text color="$title" typography="h6">
                            Figma Plugin
                        </Text>
                        <Text color="$text" typography="body">
                            A Figma plugin enabling safer and faster development.{" "}
                        </Text>
                    </VStack>
                    <Box left="467px" pos="absolute" top="20px">
                        <FigmaButton />
                    </Box>
                    <Box left="286px" pos="absolute" top="10px">
                        <FigmaButton />
                    </Box>
                </Flex>
            </VStack>
        </VStack>
    </VStack>
    <VStack
        alignItems="center"
        pb="100px"
        pt="40px"
        px="16px"
        w="100%"
    >
        <Flex
            alignItems="center"
            bg="#CDE2FA"
            borderRadius="20px 20px 0"
            justifyContent="flex-end"
            maxW="1440px"
            overflow="hidden"
            pos="relative"
            px="40px"
            py="50px"
            w="100%"
        >
            <Image
                filter="blur(4px)"
                left="-164px"
                opacity="0.8"
                pos="absolute"
                src="/icons/Group 2.svg"
                top="150px"
                w="100%"
            />
            <VStack alignItems="flex-end" flex="1" gap="80px" justifyContent="center">
                <VStack alignItems="flex-end" gap="16px" justifyContent="center">
                    <Text color="$text" textAlign="center" typography="h4">
                        Join our community
                    </Text>
                    <Text color="$text" textAlign="center" typography="textL">
                        Join our Discord and help build the future of frontend with <br />CSS-in-JS!{" "}
                    </Text>
                </VStack>
                <VStack gap="10px">
                    <BlueButton Property1="button" />
                    <BlueButton Property1="button" />
                </VStack>
            </VStack>
        </Flex>
    </VStack>
    <VStack
        bg="$footerBg"
        justifyContent="center"
        px="30px"
        py="40px"
        w="100%"
    >
        <Box display="none" justifyContent="space-between" w="1520px">
            <VStack flex="1" gap="20px" minW="240px">
                <Text color="$footerTitle" typography="buttonS">
                    Docs
                </Text>
                <VStack gap="14px">
                    <Text color="$footerText" typography="footerMenu">
                        Overview
                    </Text>
                    <Text color="$footerText" typography="footerMenu">
                        Installation
                    </Text>
                    <Text color="$footerText" typography="footerMenu">
                        Features
                    </Text>
                    <Text color="$footerText" typography="footerMenu">
                        API
                    </Text>
                    <Text color="$footerText" typography="footerMenu">
                        Devup
                    </Text>
                </VStack>
            </VStack>
            <VStack flex="1" gap="20px" minW="240px">
                <Text color="$footerTitle" typography="buttonS">
                    Components
                </Text>
                <Text color="$footerText" typography="footerMenu">
                    Overview
                </Text>
                <Text color="$footerText" display="none" typography="footerMenu" w="304px">
                    Layouts
                </Text>
            </VStack>
            <VStack flex="1" gap="20px" minW="240px">
                <Text color="$footerTitle" typography="buttonS">
                    Team
                </Text>
                <Text color="$footerText" typography="footerMenu">
                    Team
                </Text>
                <Text
                    color="$footerText"
                    display="none"
                    typography="footerMenu"
                    w="304px"
                    wordBreak="keep-all"
                >
                    상세 메뉴 2
                </Text>
                <Text
                    color="$footerText"
                    display="none"
                    typography="footerMenu"
                    w="304px"
                    wordBreak="keep-all"
                >
                    상세 메뉴 3
                </Text>
                <Text
                    color="$footerText"
                    display="none"
                    typography="footerMenu"
                    w="304px"
                    wordBreak="keep-all"
                >
                    상세 메뉴 4
                </Text>
            </VStack>
            <Box
                display="none"
                flexDir="column"
                gap="20px"
                minW="240px"
                w="304px"
            >
                <Text color="$footerNavTitle" typography="footerxl" wordBreak="keep-all">
                    메뉴 타이틀 4
                </Text>
                <Text color="$footerBody" typography="footerList" wordBreak="keep-all">
                    상세 메뉴 1
                </Text>
                <Text color="$footerBody" typography="footerList" wordBreak="keep-all">
                    상세 메뉴 2
                </Text>
                <Text color="$footerBody" typography="footerList" wordBreak="keep-all">
                    상세 메뉴 3
                </Text>
                <Text color="$footerBody" typography="footerList" wordBreak="keep-all">
                    상세 메뉴 4
                </Text>
            </Box>
            <Box
                display="none"
                flexDir="column"
                gap="20px"
                minW="240px"
                w="304px"
            >
                <Text color="$footerNavTitle" typography="footerxl" wordBreak="keep-all">
                    메뉴 타이틀 5
                </Text>
                <Text color="$footerBody" typography="footerList" wordBreak="keep-all">
                    상세 메뉴 1
                </Text>
                <Text color="$footerBody" typography="footerList" wordBreak="keep-all">
                    상세 메뉴 2
                </Text>
                <Text color="$footerBody" typography="footerList" wordBreak="keep-all">
                    상세 메뉴 3
                </Text>
                <Text color="$footerBody" typography="footerList" wordBreak="keep-all">
                    상세 메뉴 4
                </Text>
                <Text color="$footerBody" typography="footerList" wordBreak="keep-all">
                    상세 메뉴 5
                </Text>
            </Box>
        </Box>
        <VStack gap="20px">
            <Flex alignItems="flex-end">
                <Image
                    aspectRatio="4.76"
                    h="30px"
                    objectFit="contain"
                    src="/images/Frame 1321314530.png"
                    w="143px"
                />
                <Box display="none" gap="14px">
                    <Text color="$footerLink" typography="footerMenu">
                        Docs
                    </Text>
                    <Text color="$footerLink" typography="footerMenu">
                        Team
                    </Text>
                    <Text color="$footerLink" display="none" typography="footerMenu" wordBreak="keep-all">
                        서브 메뉴 3
                    </Text>
                    <Text color="$footerLink" display="none" typography="footerMenu" wordBreak="keep-all">
                        서브 메뉴 4
                    </Text>
                </Box>
            </Flex>
            <VStack gap="10px" justifyContent="center">
                <Text color="$footerText" textAlign="right" typography="small" wordBreak="keep-all">
                    상호: (주)데브파이브 | 대표자명: 오정민 | <br />사업자등록번호: 868-86-03159<br />주소: 경기 고양시 덕양구 마상로140번길 81 4층
                </Text>
                <Text color="$footerTitle" textAlign="right" typography="small" wordBreak="keep-all">
                    Copyright © 2021-2024 데브파이브. All Rights Reserved.{" "}
                </Text>
            </VStack>
        </VStack>
    </VStack>
</Center>
