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
    <Flex
        alignItems="center"
        justifyContent="space-between"
        left="0px"
        overflow="hidden"
        pos="absolute"
        px="20px"
        top="0px"
        w="100%"
    >
        <Flex
            alignItems="center"
            flex="1"
            justifyContent="space-between"
            maxW="1640px"
            w="100%"
        >
            <Flex alignItems="center" gap="30px">
                <Box
                    aspectRatio="14"
                    bg="#FFF"
                    h="20px"
                    maskImage="url(/icons/Logo.svg)"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                    w="280px"
                />
                <Flex alignItems="center" gap="30px">
                    <Box bg="#FFF" h="24px" opacity="0.2" w="2px" />
                    <Center py="8px">
                        <Text color="#FFF" typography="headerMenu">
                            SWING EZ
                        </Text>
                    </Center>
                    <Center py="8px">
                        <Text color="#FFF" typography="headerMenu">
                            VTrack
                        </Text>
                    </Center>
                    <Center borderBottom="solid 3px #FFF" h="38px" py="8px">
                        <Text color="#FFF" typography="headerMenu">
                            Notice
                        </Text>
                    </Center>
                    <Center py="8px">
                        <Text color="#FFF" typography="headerMenu">
                            Contact
                        </Text>
                    </Center>
                </Flex>
            </Flex>
            <Flex
                alignItems="center"
                borderRadius="100px"
                gap="8px"
                px="12px"
                py="8px"
                w="111px"
            >
                <Box
                    aspectRatio="1"
                    bg="#FFF"
                    boxSize="24px"
                    maskImage="url(/icons/grommet-icons:language.svg)"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
                <Text color="#FFF" typography="headerMenuSm">
                    KOR
                </Text>
                <Box
                    aspectRatio="1"
                    bg="#FFF"
                    boxSize="16px"
                    maskImage="url(/icons/grommet-icons:language.svg)"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            </Flex>
        </Flex>
    </Flex>
    <VStack alignItems="center" overflow="hidden" px="30px" py="40px">
        <VStack alignItems="center" gap="30px" maxW="1280px" w="100%">
            <Flex alignItems="center" gap="40px" w="100%">
                <Flex flex="1" gap="48px">
                    <Flex gap="4px" justifyContent="center">
                        <Text color="$text" typography="noticeSelected" wordBreak="keep-all">
                            전체
                        </Text>
                        <Box
                            bg="$primary"
                            boxSize="8px"
                            maskImage="url('/icons/Frame 1000014290.svg')"
                            maskPos="center"
                            maskRepeat="no-repeat"
                            maskSize="contain"
                        />
                    </Flex>
                    <Flex justifyContent="center" opacity="0.55">
                        <Text color="$text" typography="noticeMenu" wordBreak="keep-all">
                            공지
                        </Text>
                    </Flex>
                    <Flex justifyContent="center" opacity="0.55">
                        <Text color="$text" typography="noticeMenu" wordBreak="keep-all">
                            이벤트
                        </Text>
                    </Flex>
                    <Flex justifyContent="center" opacity="0.55">
                        <Text color="$text" typography="noticeMenu" wordBreak="keep-all">
                            업데이트
                        </Text>
                    </Flex>
                    <Flex justifyContent="center" opacity="0.55">
                        <Text color="$text" typography="noticeMenu" wordBreak="keep-all">
                            매뉴얼
                        </Text>
                    </Flex>
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
                    <Box
                        aspectRatio="1"
                        bg="$primary"
                        boxSize="32px"
                        maskImage="url(/icons/icons.svg)"
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
                <Center
                    aspectRatio="1"
                    bg="$innerBg"
                    border="solid 2px $primary"
                    borderRadius="1000px"
                    boxSize="40px"
                    flexDir="column"
                >
                    <Text color="$primary" typography="pagination">
                        1
                    </Text>
                </Center>
            </Flex>
        </VStack>
    </VStack>
    <Flex bg="#2D2926" justifyContent="center" overflow="hidden" p="60px">
        <Flex flex="1" gap="80px" maxW="1280px" w="100%">
            <Flex gap="80px">
                <VStack gap="20px">
                    <Box
                        aspectRatio="14"
                        bg="#FFF"
                        h="16px"
                        maskImage="url(/icons/Logo.svg)"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                        w="220px"
                    />
                    <Flex gap="16px">
                        <Flex
                            alignItems="center"
                            bg="#FFF"
                            borderRadius="1000px"
                            overflow="hidden"
                            p="2px"
                            w="64px"
                        >
                            <Flex
                                aspectRatio="1"
                                bg="#2D2926"
                                borderRadius="10000px"
                                boxSize="28px"
                                p="4px"
                            >
                                <Box
                                    aspectRatio="1"
                                    bg="#FFF"
                                    boxSize="20px"
                                    maskImage="url(/icons/light.svg)"
                                    maskPos="center"
                                    maskRepeat="no-repeat"
                                    maskSize="contain"
                                />
                            </Flex>
                        </Flex>
                        <Box
                            bg="#FFF"
                            h="32px"
                            maskImage="url('/icons/Frame 1000014232.svg')"
                            maskPos="center"
                            maskRepeat="no-repeat"
                            maskSize="contain"
                        />
                    </Flex>
                </VStack>
                <VStack gap="10px">
                    <Text color="#FFF" typography="footerTitle" wordBreak="keep-all">
                        라온피플(주)
                    </Text>
                    <Text color="#FFF" opacity="0.5" typography="footerText" wordBreak="keep-all">
                        대표이사 : 이석중 주소 : 13840 경기 과천시 과천대로7나길 60 과천어반허브, C동 5층/6층<br />TEL : 1899-3058<br />FAX : 02-3318-3351<br />이메일 : sales@laonpeople.com{" "}
                    </Text>
                </VStack>
            </Flex>
            <Flex flex="1" gap="30px" justifyContent="flex-end">
                <Text color="#FFF" typography="footerMenu">
                    SWING EZ
                </Text>
                <Text color="#FFF" typography="footerMenu">
                    VTrack
                </Text>
                <Text color="#FFF" typography="footerMenu">
                    Notice
                </Text>
                <Text color="#FFF" typography="footerMenu">
                    Contact
                </Text>
            </Flex>
        </Flex>
    </Flex>
</VStack>
