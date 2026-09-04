<VStack bg="$containerBackground">
    <VStack
        alignItems="center"
        bg="$primary"
        gap="10px"
        h="320px"
        justifyContent="flex-end"
        overflow="hidden"
        pos="relative"
        px="30px"
        py="20px"
    >
        <VStack maxW="1280px" w="100%">
            <Flex alignItems="center" gap="11px">
                <Text color="#FFF" typography="mainBannerExbold">
                    Notice
                </Text>
                <Box bg="#FFF" h="2px" w="92px" />
            </Flex>
            <Text color="#FFF" typography="h2Exbold" wordBreak="keep-all">
                공지사항
            </Text>
        </VStack>
        <Box
            bg="#FFF"
            left="50%"
            maskImage="url('/icons/Frame 1000014364.svg')"
            maskPos="center"
            maskRepeat="no-repeat"
            maskSize="contain"
            pos="absolute"
            top="172px"
            transform="translateX(-50%)"
            w="100%"
        />
    </VStack>
    <Box left="0px" pos="absolute" top="0px" w="100%">
        <Header property1="transparent" />
    </Box>
    <VStack alignItems="center" overflow="hidden" px="30px" py="40px">
        <VStack alignItems="center" gap="30px" maxW="1280px" w="100%">
            <Flex alignItems="center" gap="40px" w="100%">
                <Flex flex="1" gap="48px">
                    <Tab />
                    <Tab />
                    <Tab />
                    <Tab />
                    <Tab />
                </Flex>
                <Flex
                    alignItems="center"
                    bg="$background"
                    borderRadius="100px"
                    justifyContent="space-between"
                    px="24px"
                    py="10px"
                    w="300px"
                >
                    <Text color="$text" typography="noticeSearch" wordBreak="keep-all">
                        라멘집
                    </Text>
                    {/* <Icons Property1="search" /> */}
                    <Box
                        bg="$text"
                        boxSize="32px"
                        maskImage="url('/icons/Property 1=search.svg')"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                    />
                </Flex>
            </Flex>
            <VStack borderTop="solid 2px $primary" w="100%">
                <Center
                    bg="$innerBg"
                    borderBottom="solid 1px $border"
                    flexDir="column"
                    gap="20px"
                    px="24px"
                    py="80px"
                >
                    <Image h="95px" src="/icons/Frame 1321314514.svg" w="100px" />
                    <VStack alignItems="center">
                        <Flex alignItems="center">
                            <Text color="$primary" typography="bodySb" wordBreak="keep-all">
                                ‘라멘집’
                            </Text>
                            <Text color="$text" typography="body" wordBreak="keep-all">
                                {" "}검색 결과가 없습니다.
                            </Text>
                        </Flex>
                        <Text color="$text" typography="body" wordBreak="keep-all">
                            검색어가 올바른지 확인해주세요.
                        </Text>
                    </VStack>
                </Center>
            </VStack>
            <Flex>
                <Pagination />
            </Flex>
        </VStack>
    </VStack>
    <Footer property1="desktop" />
</VStack>
