import { Box, Center, Flex, Image, Text, VStack } from '@devup-ui/react'
import { Footer } from '@/components/Footer'
import { Header } from '@/components/Header'
import { Pagination } from '@/components/Pagination'
import { Tab } from '@/components/Tab'

export default function NoticePage() {
    return (
        <VStack bg="$containerBackground">
            <VStack
                alignItems="center"
                bg="$primary"
                display={[
                    "none",
                    null,
                    "flex"
                ]}
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
            <Box
                bg="$primary"
                display={[
                    null,
                    null,
                    "none"
                ]}
                h="240px"
                overflow="hidden"
                pos="relative"
            >
                <VStack bottom="20px" left="20px" pos="absolute">
                    <Flex alignItems="center" gap="11px">
                        <Text color="#FFF" typography="mainBannerExbold">
                            Notice
                        </Text>
                        <Box bg="#FFF" h="2px" w="40px" />
                    </Flex>
                    <Text color="#FFF" typography="h2Exbold" wordBreak="keep-all">
                        공지사항
                    </Text>
                </VStack>
                <Box left="44px" pos="absolute" top="169px">
                    <Box
                        bg="$text"
                        h="46px"
                        maskImage="url(/icons/Logo.svg)"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                        w="651px"
                    />
                </Box>
                <Box left="-212px" pos="absolute" top="112px">
                    <Box
                        bg="$text"
                        h="46px"
                        maskImage="url(/icons/Logo.svg)"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                        w="651px"
                    />
                </Box>
            </Box>
            <Box left="0px" pos="absolute" top="0px" w="100%">
                <Header property1="transparent" />
            </Box>
            <VStack
                alignItems="center"
                display={[
                    "none",
                    null,
                    "flex"
                ]}
                overflow="hidden"
                px="30px"
                py="40px"
            >
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
            <VStack
                alignItems="center"
                display={[
                    "flex",
                    null,
                    "none"
                ]}
                overflow="hidden"
                px="16px"
                py="32px"
            >
                <VStack alignItems="center" gap="30px" maxW="1280px" w="100%">
                    <VStack gap="24px" justifyContent="center" w="100%">
                        <Flex
                            alignItems="center"
                            bg="$background"
                            borderRadius="100px"
                            justifyContent="space-between"
                            px="24px"
                            py="10px"
                        >
                            <Text color="$text" typography="noticeSearch" wordBreak="keep-all">
                                라멘집
                            </Text>
                            <Box
                                bg="$text"
                                boxSize="32px"
                                maskImage="url('/icons/Property 1=search.svg')"
                                maskPos="center"
                                maskRepeat="no-repeat"
                                maskSize="contain"
                            />
                        </Flex>
                        <Flex gap="32px" px="8px">
                            <Tab />
                            <Tab />
                            <Tab />
                            <Tab />
                            <Tab />
                        </Flex>
                    </VStack>
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
                                    <Text color="$primary" typography="bodyBold" whiteSpace="nowrap" wordBreak="keep-all">
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
                    <Flex gap="8px">
                        <Pagination />
                        <Pagination />
                        <Pagination />
                    </Flex>
                </VStack>
            </VStack>
            <Footer property1="desktop" />
        </VStack>
    )
}
