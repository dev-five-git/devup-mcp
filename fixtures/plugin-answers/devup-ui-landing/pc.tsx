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
            <VStack gap="20px">
                <Flex alignItems="flex-end" gap="20px" justifyContent="center">
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
                                1.0.18
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
                                        18.23s
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
                                        54.75MB
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
                                3.27.0
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" gap="20px">
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Time
                                </Text>
                                <Text color="$caption" typography="h5">
                                    29.99s
                                </Text>
                            </VStack>
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Size
                                </Text>
                                <Text color="$caption" typography="h5">
                                    200.39MB
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
                                7.3.2
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" gap="20px">
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Time
                                </Text>
                                <Text color="$caption" typography="h5">
                                    22.21s
                                </Text>
                            </VStack>
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Size
                                </Text>
                                <Text color="$caption" typography="h5">
                                    89.87MB
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
                                    21.61s
                                </Text>
                            </VStack>
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Size
                                </Text>
                                <Text color="$caption" typography="h5">
                                    64.30MB
                                </Text>
                            </VStack>
                        </VStack>
                    </VStack>
                </Flex>
                <Flex alignItems="center" gap="20px" px="40px">
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
                                Tailwind CSS
                            </Text>
                            <Text color="$captionBold" typography="textL">
                                4.1.13
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" gap="20px">
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Time
                                </Text>
                                <Text color="$caption" typography="h5">
                                    20.22s
                                </Text>
                            </VStack>
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Size
                                </Text>
                                <Text color="$caption" typography="h5">
                                    54.76MB
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
                                panda CSS
                            </Text>
                            <Text color="$captionBold" typography="textL">
                                1.3.1
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" gap="20px">
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Time
                                </Text>
                                <Text color="$caption" typography="h5">
                                    22.01s
                                </Text>
                            </VStack>
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Size
                                </Text>
                                <Text color="$caption" typography="h5">
                                    59.53MB
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
                                styleX
                            </Text>
                            <Text color="$captionBold" typography="textL">
                                0.15.4
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" gap="20px">
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Time
                                </Text>
                                <Text color="$caption" typography="h5">
                                    38.97s
                                </Text>
                            </VStack>
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Size
                                </Text>
                                <Text color="$caption" typography="h5">
                                    72.72MB
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
                                vanilla extract
                            </Text>
                            <Text color="$captionBold" typography="textL">
                                1.17.4
                            </Text>
                        </VStack>
                        <VStack alignItems="flex-end" gap="20px">
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Time
                                </Text>
                                <Text color="$caption" typography="h5">
                                    20.09s
                                </Text>
                            </VStack>
                            <VStack alignItems="flex-end" gap="6px" justifyContent="center">
                                <Text color="$captionBold" typography="textSbold">
                                    Bulid Size
                                </Text>
                                <Text color="$caption" typography="h5">
                                    56.61MB
                                </Text>
                            </VStack>
                        </VStack>
                    </VStack>
                </Flex>
            </VStack>
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
                        {/* <010.idea style="flat" /> */}
                        <Image boxSize="32px" src="/icons/style=flat.svg" />
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
                        {/* <019.trophy style="flat" /> */}
                        <Box
                            bg="#FFC738"
                            boxSize="32px"
                            maskImage="url(/icons/style=flat.svg)"
                            maskPos="center"
                            maskRepeat="no-repeat"
                            maskSize="contain"
                        />
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
                        {/* <021.heart style="flat" /> */}
                        <Box
                            bg="#F4868F"
                            boxSize="32px"
                            maskImage="url(/icons/style=flat.svg)"
                            maskPos="center"
                            maskRepeat="no-repeat"
                            maskSize="contain"
                        />
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
                        {/* <016.notice style="flat" /> */}
                        <Image boxSize="32px" src="/icons/style=flat.svg" />
                        <VStack gap="10px">
                            <Text color="$title" typography="h6">
                                Figma Plugin
                            </Text>
                            <Text color="$text" typography="body">
                                A Figma plugin enabling safer and faster development.{" "}
                            </Text>
                        </VStack>
                        <Box pos="absolute" right="20px" top="20px">
                            <FigmaButton />
                        </Box>
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
                    <BlueButton Property1="button" />
                    <BlueButton Property1="button" />
                </Flex>
            </VStack>
        </Flex>
    </VStack>
    <Footer />
    <Box left="0px" pos="absolute" top="0px" w="100%">
        <Gnb Property1="Default" />
    </Box>
</Center>
