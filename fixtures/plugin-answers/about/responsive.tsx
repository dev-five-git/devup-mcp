import { Box, Center, Flex, Image, Text, VStack } from '@devup-ui/react'
import { Footer } from '@/components/Footer'
import { Header } from '@/components/Header'

export default function AboutPage() {
    return (
        <VStack bg="$containerBackground" overflow="hidden">
            <Box
                display="none"
                h="667px"
                left="50%"
                opacity="0.8"
                overflow="hidden"
                pos="absolute"
                top="64px"
                transform="translateX(-50%)"
                w="100%"
            />
            <Header status="landing" />
            <Center
                bg="linear-gradient(#FFFFFFB2, #FFFFFFB2), url(/icons/image.png) center/cover no-repeat"
                display={[
                    "flex",
                    null,
                    "none"
                ]}
                flexDir="column"
                h="520px"
                overflow="hidden"
                px="20px"
                py="40px"
            >
                <VStack
                    alignItems="flex-end"
                    gap="20px"
                    justifyContent="flex-end"
                    maxW="1280px"
                    pos="relative"
                    w="100%"
                >
                    <Image
                        boxSize="100%"
                        left="50%"
                        pos="absolute"
                        src="/images/제목 없음-2 1.png"
                        top="-107px"
                        transform="translateX(-50%)"
                    />
                    <Text color="$title" textAlign="right" typography="h3" wordBreak="keep-all">
                        <Text color="$secondary">
                            성인 ADHD,{"  "}
                        </Text>
                        우리는 다르게 봅니다.
                    </Text>
                    <Text color="$title" textAlign="right" typography="bodyXlg" wordBreak="keep-all">
                        퍼즐핏은 당사자의 경험에서 출발한, <br />성인 ADHD 전문 플랫폼입니다.
                    </Text>
                </VStack>
            </Center>
            <Center
                bg="linear-gradient(#FFFFFFB2, #FFFFFFB2), url(/icons/image.png) center/cover no-repeat"
                display={[
                    "none",
                    null,
                    "flex"
                ]}
                flexDir="column"
                h="400px"
                overflow="hidden"
                px="30px"
                py="80px"
            >
                <VStack
                    alignItems="flex-end"
                    gap="20px"
                    justifyContent="center"
                    maxW="1280px"
                    pos="relative"
                    w="100%"
                >
                    <Image
                        h="100%"
                        left="-138px"
                        pos="absolute"
                        src="/images/제목 없음-2 1.png"
                        top="-244.5px"
                        w={[
                            "770px",
                            null,
                            "778px",
                            null,
                            "770px"
                        ]}
                    />
                    <Text
                        color="$title"
                        textAlign={[
                            null,
                            null,
                            "right",
                            null,
                            "initial"
                        ]}
                        typography="h3"
                        wordBreak="keep-all"
                    >
                        <Text color="$secondary"><br />  성인 ADHD,{"  "}<br /></Text>우리는 다르게 봅니다.
                    </Text>
                    <Text
                        color="$title"
                        textAlign={[
                            null,
                            null,
                            "right",
                            null,
                            "initial"
                        ]}
                        typography="bodyXlg"
                        wordBreak="keep-all"
                    >
                        퍼즐핏은 당사자의 경험에서 출발한, 성인 ADHD 전문 플랫폼입니다.
                    </Text>
                </VStack>
            </Center>
            <Center
                display={[
                    "flex",
                    null,
                    "none"
                ]}
                flexDir="column"
                overflow="hidden"
                px="20px"
                py="50px"
            >
                <Center flexDir="column" maxW="1280px" w="100%">
                    <VStack gap="20px" w="100%">
                        <VStack justifyContent="center">
                            <Text color="$primary" typography="title">
                                Puzzlefit’s
                            </Text>
                            <Text color="$title" typography="h4">
                                Story
                            </Text>
                        </VStack>
                        <VStack gap="20px">
                            <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                                저는 성인 ADHD 당사자이자 정신건강간호사입니다. 진단을 받기까지 수년이 걸렸고, 그 과정에서 수많은 좌절과 시행착오를 겪었습니다. 그 경험을 통해 알게 된 것이 있습니다.
                            </Text>
                            <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                                성인 ADHD는 단순히 증상으로만 정의되지 않습니다.
                                {" "}같은 어려움을 겪더라도 각자가 만들어온{" "}
                                대처 기술, 사고방식, 반응 방식은 모두 다릅니다.{" "}
                                {" "}전문가조차 완전히 이해하기 어려운, 오직 당사자만이 체감할 수 있는 영역이 존재합니다.{"  "}
                                퍼즐핏은 그 목소리에서 출발했습니다.
                                {"  "}우리는 단순한 진단 도구를 만드는 것이 아니라, 당사자가 스스로를 이해하고 사회 속에서 더 잘 기능할 수 있도록 돕는 시스템을 만들고 있습니다
                            </Text>
                        </VStack>
                    </VStack>
                </Center>
            </Center>
            <Center
                display={[
                    "none",
                    null,
                    "flex"
                ]}
                flexDir="column"
                overflow="hidden"
                px="30px"
                py={[
                    "120px",
                    null,
                    "100px",
                    null,
                    "120px"
                ]}
            >
                <Flex
                    gap={[
                        "60px",
                        null,
                        "40px",
                        null,
                        "60px"
                    ]}
                    maxW="1280px"
                    w="100%"
                >
                    <Image
                        borderRadius="20px"
                        flex="1"
                        h="540px"
                        maxW="460px"
                        src="/images/Frame 269.png"
                        w="100%"
                    />
                    <VStack
                        flex="1"
                        gap={[
                            "50px",
                            null,
                            "30px",
                            null,
                            "50px"
                        ]}
                    >
                        <VStack gap="10px">
                            <Text color="$primary" typography="title">
                                Puzzlefit’s
                            </Text>
                            <Text color="$title" typography="h4">
                                Story
                            </Text>
                        </VStack>
                        <VStack gap="20px">
                            <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                                저는 성인 ADHD 당사자이자 정신건강간호사입니다. 진단을 받기까지 수년이 걸렸고, 그 과정에서 수많은 좌절과 시행착오를 겪었습니다. 그 경험을 통해 알게 된 것이 있습니다.
                            </Text>
                            <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                                성인 ADHD는 단순히 증상으로만 정의되지 않습니다.
                                {"  "}같은 어려움을 겪더라도 각자가 만들어온{" "}
                                대처 기술, 사고방식, 반응 방식은 모두 다릅니다.
                                {"  "}전문가조차 완전히 이해하기 어려운, 오직 당사자만이 체감할 수 있는 영역이 존재합니다.{"  "}
                                퍼즐핏은 그 목소리에서 출발했습니다.
                                {"  "}우리는 단순한 진단 도구를 만드는 것이 아니라, 당사자가 스스로를 이해하고 사회 속에서 더 잘 기능할 수 있도록 돕는 시스템을 만들고 있습니다
                            </Text>
                        </VStack>
                    </VStack>
                </Flex>
            </Center>
            <Image
                display={[
                    null,
                    null,
                    "none"
                ]}
                h="240px"
                src="/images/Frame 269.png"
            />
            <Center
                display={[
                    "none",
                    null,
                    null,
                    null,
                    "flex"
                ]}
                flexDir="column"
                overflow="hidden"
                pb="100px"
                pt="80px"
                px="30px"
            >
                <Flex gap="50px" maxW="1280px" w="100%">
                    <Text color="$title" typography="h4" w="240px">
                        Mission
                    </Text>
                    <VStack gap="50px" w="1079px">
                        <Flex gap="40px" maxW="740px" w="100%">
                            <Center
                                aspectRatio="1"
                                bg="$primaryBgLight"
                                borderRadius="2000px"
                                flex="1"
                                flexDir="column"
                                gap="16px"
                                h="220px"
                                overflow="hidden"
                                pt="12px"
                            >
                                <Box
                                    aspectRatio="1"
                                    bg="$primary"
                                    boxSize="52px"
                                    maskImage="url('/icons/file-analytic_14427169 1.svg')"
                                    maskPos="center"
                                    maskRepeat="no-repeat"
                                    maskSize="contain"
                                />
                                <Text color="$primary" typography="bodyLgBold" wordBreak="keep-all">
                                    연구기반 자가 검진
                                </Text>
                            </Center>
                            <Center
                                aspectRatio="1"
                                bg="$primaryBgLight"
                                borderRadius="2000px"
                                flex="1"
                                flexDir="column"
                                gap="16px"
                                h="220px"
                                overflow="hidden"
                                pt="12px"
                            >
                                <Image aspectRatio="1" boxSize="52px" src="/icons/shopping-list_1288709 1.svg" />
                                <Text color="$primary" typography="bodyLgBold" wordBreak="keep-all">
                                    맞춤형 보고서
                                </Text>
                            </Center>
                            <Center
                                aspectRatio="1"
                                bg="$primaryBgLight"
                                borderRadius="2000px"
                                flex="1"
                                flexDir="column"
                                gap="16px"
                                h="220px"
                                overflow="hidden"
                                pt="12px"
                            >
                                <Box
                                    aspectRatio="1"
                                    bg="$primary"
                                    boxSize="52px"
                                    maskImage="url('/icons/meeting_1621655 1.svg')"
                                    maskPos="center"
                                    maskRepeat="no-repeat"
                                    maskSize="contain"
                                />
                                <Text color="$primary" typography="bodyLgBold" wordBreak="keep-all">
                                    코칭 {"&"} 커뮤니티
                                </Text>
                            </Center>
                        </Flex>
                        <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                            우리는 단순한 검사 도구를 만드는 것이 아닙니다. 퍼즐핏은 연구 기반의 자가검진, 맞춤형 보고서, 코칭과 커뮤니티를 통해  <br />당사자가 사회 속에서 더 잘 기능할 수 있도록 돕는 시스템을 구축하고 있습니다.
                        </Text>
                    </VStack>
                </Flex>
            </Center>
            <Center
                display={[
                    "none",
                    null,
                    "flex",
                    null,
                    "none"
                ]}
                flexDir="column"
                overflow="hidden"
                px="30px"
                py="80px"
            >
                <Center flexDir="column" gap="50px" maxW="1280px" w="100%">
                    <Text color="$title" typography="h4">
                        Mission
                    </Text>
                    <VStack alignItems="center" gap="50px" w="100%">
                        <Flex gap="40px">
                            <Center
                                bg="$primaryBgLight"
                                borderRadius="2000px"
                                boxSize="220px"
                                flexDir="column"
                                gap="16px"
                                overflow="hidden"
                                pt="12px"
                            >
                                <Box
                                    aspectRatio="1"
                                    bg="$primary"
                                    boxSize="52px"
                                    maskImage="url('/icons/file-analytic_14427169 1.svg')"
                                    maskPos="center"
                                    maskRepeat="no-repeat"
                                    maskSize="contain"
                                />
                                <Text color="$primary" typography="bodyLgBold" wordBreak="keep-all">
                                    연구기반 자가 검진
                                </Text>
                            </Center>
                            <Center
                                bg="$primaryBgLight"
                                borderRadius="2000px"
                                boxSize="220px"
                                flexDir="column"
                                gap="16px"
                                overflow="hidden"
                                pt="12px"
                            >
                                <Image aspectRatio="1" boxSize="52px" src="/icons/shopping-list_1288709 1.svg" />
                                <Text color="$primary" typography="bodyLgBold" wordBreak="keep-all">
                                    맞춤형 보고서
                                </Text>
                            </Center>
                            <Center
                                bg="$primaryBgLight"
                                borderRadius="2000px"
                                boxSize="220px"
                                flexDir="column"
                                gap="16px"
                                overflow="hidden"
                                pt="12px"
                            >
                                <Box
                                    aspectRatio="1"
                                    bg="$primary"
                                    boxSize="52px"
                                    maskImage="url('/icons/meeting_1621655 1.svg')"
                                    maskPos="center"
                                    maskRepeat="no-repeat"
                                    maskSize="contain"
                                />
                                <Text color="$primary" typography="bodyLgBold" wordBreak="keep-all">
                                    코칭 {"&"} 커뮤니티
                                </Text>
                            </Center>
                        </Flex>
                        <Text color="$text" textAlign="center" typography="bodyXlg" wordBreak="keep-all">
                            우리는 단순한 검사 도구를 만드는 것이 아닙니다. 퍼즐핏은 연구 기반의 자가검진, 맞춤형 보고서, 코칭과 커뮤니티를 통해  <br />당사자가 사회 속에서 더 잘 기능할 수 있도록 돕는 시스템을 구축하고 있습니다.
                        </Text>
                    </VStack>
                </Center>
            </Center>
            <Center
                display={[
                    "flex",
                    null,
                    "none"
                ]}
                flexDir="column"
                overflow="hidden"
                pb="80px"
                pt="40px"
                px="20px"
            >
                <Center flexDir="column" gap="50px" maxW="1280px" w="100%">
                    <Text color="$title" typography="h4">
                        Mission
                    </Text>
                    <VStack alignItems="center" gap="50px" w="100%">
                        <VStack gap="30px">
                            <Center
                                aspectRatio="1"
                                bg="$primaryBgLight"
                                borderRadius="2000px"
                                boxSize="170px"
                                flexDir="column"
                                gap="16px"
                                overflow="hidden"
                                pt="12px"
                            >
                                <Box
                                    aspectRatio="1"
                                    bg="$primary"
                                    boxSize="36px"
                                    maskImage="url('/icons/file-analytic_14427169 1.svg')"
                                    maskPos="center"
                                    maskRepeat="no-repeat"
                                    maskSize="contain"
                                />
                                <Text color="$primary" typography="bodyLgBold" wordBreak="keep-all">
                                    연구기반 자가 검진
                                </Text>
                            </Center>
                            <Center
                                aspectRatio="1"
                                bg="$primaryBgLight"
                                borderRadius="2000px"
                                boxSize="170px"
                                flexDir="column"
                                gap="16px"
                                overflow="hidden"
                                pt="12px"
                            >
                                <Image aspectRatio="1" boxSize="36px" src="/icons/shopping-list_1288709 1.svg" />
                                <Text color="$primary" typography="bodyLgBold" wordBreak="keep-all">
                                    맞춤형 보고서
                                </Text>
                            </Center>
                            <Center
                                aspectRatio="1"
                                bg="$primaryBgLight"
                                borderRadius="2000px"
                                boxSize="170px"
                                flexDir="column"
                                gap="16px"
                                overflow="hidden"
                                pt="12px"
                            >
                                <Box
                                    aspectRatio="1"
                                    bg="$primary"
                                    boxSize="36px"
                                    maskImage="url('/icons/meeting_1621655 1.svg')"
                                    maskPos="center"
                                    maskRepeat="no-repeat"
                                    maskSize="contain"
                                />
                                <Text color="$primary" typography="bodyLgBold" wordBreak="keep-all">
                                    코칭 {"&"} 커뮤니티
                                </Text>
                            </Center>
                        </VStack>
                        <Text
                            color="$text"
                            textAlign="center"
                            typography="bodyXlg"
                            w="100%"
                            wordBreak="keep-all"
                        >
                            우리는 단순한 검사 도구를 만드는 것이 아닙니다. 퍼즐핏은 연구 기반의 자가검진, 맞춤형 보고서, <br />코칭과 커뮤니티를 통해 당사자가 사회 속에서 더 잘 기능할 수 있도록 돕는 시스템을 구축하고 <br />있습니다.
                        </Text>
                    </VStack>
                </Center>
            </Center>
            <Center
                bg="$gray100"
                display={[
                    "none",
                    null,
                    null,
                    null,
                    "flex"
                ]}
                flexDir="column"
                overflow="hidden"
                px="30px"
                py="80px"
            >
                <Flex gap="50px" maxW="1280px" pos="relative" w="100%">
                    <Text color="$title" typography="h4" w="240px">
                        Vision
                    </Text>
                    <VStack flex="1" gap="40px">
                        <Flex gap="20px">
                            <Box
                                aspectRatio="1.23"
                                bg="$secondary"
                                h="14px"
                                maskImage="url(/icons/“.svg)"
                                maskPos="center"
                                maskRepeat="no-repeat"
                                maskSize="contain"
                                opacity="0.3"
                                w="17px"
                            />
                            <Text color="$secondary" typography="title" wordBreak="keep-all">
                                ADHD를 약점이 아닌 가능성으로 보는 세상
                            </Text>
                            <Box
                                aspectRatio="1.23"
                                bg="$secondary"
                                h="14px"
                                maskImage="url(/icons/“.svg)"
                                maskPos="center"
                                maskRepeat="no-repeat"
                                maskSize="contain"
                                opacity="0.3"
                                transform="rotate(180deg)"
                                w="17px"
                            />
                        </Flex>
                        <Center pl="32px">
                            <Text color="$text" flex="1" typography="bodyXlg" wordBreak="keep-all">
                                퍼즐핏이 바라는 미래는 <br />성인 ADHD가 ‘진단명’으로만 불리는 것이 아니라,{" "}
                                <Text typography="bodyXlgBold">
                                    자기 이해와 성장의 한 부분
                                </Text>
                                으로 받아들여지는 세상.<br /><br />당사자들이 더 이상 혼자가 아니라, <br />자신의 퍼즐 조각을 찾아 온전한 그림을 완성해 나가는 세상.<br />그 길을 함께 만들어가고 싶습니다.<br /><br />퍼즐핏 대표{" "}
                                <Text typography="bodyXlgBold">
                                    맹소영
                                </Text>
                                {" "}드림
                            </Text>
                        </Center>
                    </VStack>
                    <Box
                        bg="$gray300"
                        maskImage="url('/icons/puzzle_17833563 1.svg')"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                        opacity="0.2"
                        pos="absolute"
                        right="-100px"
                        top="46px"
                        w="465px"
                    />
                </Flex>
            </Center>
            <Center
                bg="$gray100"
                display={[
                    "none",
                    null,
                    "flex",
                    null,
                    "none"
                ]}
                flexDir="column"
                overflow="hidden"
                p="80px"
            >
                <VStack gap="50px" maxW="1280px" pos="relative" w="100%">
                    <Text color="$title" typography="h4" w="240px">
                        Vision
                    </Text>
                    <VStack gap="40px">
                        <Flex gap="20px">
                            <Box
                                aspectRatio="1.23"
                                bg="$secondary"
                                h="14px"
                                maskImage="url(/icons/“.svg)"
                                maskPos="center"
                                maskRepeat="no-repeat"
                                maskSize="contain"
                                opacity="0.3"
                                w="17px"
                            />
                            <Text color="$secondary" typography="title" wordBreak="keep-all">
                                ADHD를 약점이 아닌 가능성으로 보는 세상
                            </Text>
                            <Box
                                aspectRatio="1.23"
                                bg="$secondary"
                                h="14px"
                                maskImage="url(/icons/“.svg)"
                                maskPos="center"
                                maskRepeat="no-repeat"
                                maskSize="contain"
                                opacity="0.3"
                                transform="rotate(180deg)"
                                w="17px"
                            />
                        </Flex>
                        <Center pl="32px">
                            <Text color="$text" flex="1" typography="bodyXlg" wordBreak="keep-all">
                                퍼즐핏이 바라는 미래는 <br />성인 ADHD가 ‘진단명’으로만 불리는 것이 아니라,{" "}
                                <Text typography="bodyXlgBold">
                                    자기 이해와 성장의 한 부분
                                </Text>
                                으로 받아들여지는 세상.<br /><br />당사자들이 더 이상 혼자가 아니라, <br />자신의 퍼즐 조각을 찾아 온전한 그림을 완성해 나가는 세상.<br />그 길을 함께 만들어가고 싶습니다.<br /><br />퍼즐핏 대표{" "}
                                <Text typography="bodyXlgBold">
                                    맹소영
                                </Text>
                                {" "}드림
                            </Text>
                        </Center>
                    </VStack>
                    <Box
                        bg="$gray300"
                        maskImage="url('/icons/puzzle_17833563 1.svg')"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                        opacity="0.2"
                        pos="absolute"
                        right="-100px"
                        top="46px"
                        w="465px"
                    />
                </VStack>
            </Center>
            <Center
                bg="$gray100"
                display={[
                    "flex",
                    null,
                    "none"
                ]}
                flexDir="column"
                overflow="hidden"
                px="16px"
                py="60px"
            >
                <VStack
                    alignItems="center"
                    gap="50px"
                    maxW="1280px"
                    pos="relative"
                    w="100%"
                >
                    <Box
                        bg="$gray300"
                        maskImage="url('/icons/puzzle_17833563 1.svg')"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                        opacity="0.2"
                        pos="absolute"
                        right="-100px"
                        top="157px"
                        w="100%"
                    />
                    <Text color="$title" typography="h4">
                        Vision
                    </Text>
                    <VStack alignItems="center" gap="40px" w="100%">
                        <Flex gap="10px" justifyContent="center">
                            <Box
                                aspectRatio="1.23"
                                bg="$secondary"
                                h="14px"
                                maskImage="url(/icons/“.svg)"
                                maskPos="center"
                                maskRepeat="no-repeat"
                                maskSize="contain"
                                opacity="0.3"
                                w="17px"
                            />
                            <Text color="$secondary" typography="title" wordBreak="keep-all">
                                ADHD를 약점이 아닌, <br />가능성으로 보는 세상
                            </Text>
                            <Box
                                aspectRatio="1.23"
                                bg="$secondary"
                                h="14px"
                                maskImage="url(/icons/“.svg)"
                                maskPos="center"
                                maskRepeat="no-repeat"
                                maskSize="contain"
                                opacity="0.3"
                                transform="rotate(180deg)"
                                w="17px"
                            />
                        </Flex>
                        <Center w="100%">
                            <Text
                                color="$text"
                                flex="1"
                                textAlign="center"
                                typography="bodyXlg"
                                wordBreak="keep-all"
                            >
                                퍼즐핏이 바라는 미래는 <br />성인 ADHD가 ‘진단명’으로만 불리는 것이 아니라,{" "}
                                <Text typography="bodyXlgBold">
                                    자기 이해와 성장의 한 부분
                                </Text>
                                으로 받아들여지는 세상.<br /><br />당사자들이 더 이상 혼자가 아니라, <br />자신의 퍼즐 조각을 찾아 온전한 그림을 <br />완성해 나가는 세상.<br />그 길을 함께 만들어가고 싶습니다.<br /><br />퍼즐핏 대표{" "}
                                <Text typography="bodyXlgBold">
                                    맹소영
                                </Text>
                                {" "}드림
                            </Text>
                        </Center>
                    </VStack>
                </VStack>
            </Center>
            <Center
                display={[
                    "none",
                    null,
                    null,
                    null,
                    "flex"
                ]}
                flexDir="column"
                overflow="hidden"
                pb="80px"
                pt="100px"
                px="30px"
            >
                <Flex gap="50px" maxW="1280px" w="100%">
                    <Text color="$title" typography="h4" w="240px">
                        Solution
                    </Text>
                    <VStack flex="1" gap="50px">
                        <Image borderRadius="20px" h="300px" src="/images/Frame 1000014115.png" />
                        <Text color="$text" typography="bodyXlg" w="619px" wordBreak="keep-all">
                            <Text color="$primary" typography="title">
                                전문가와 함께 만드는, 신뢰 기반의 성인 ADHD 솔루션{" "}
                            </Text>
                            {" "}퍼즐핏은 단순한 검사 도구 제공이 아닌{" "}
                            연구 기반 자가검진, 맞춤 리포트, 코칭
                            을 통해 당사자가 사회 속에서 더 잘 기능하도록 돕는 통합 솔루션입니다.{"  "}
                            ADHD 전문 의사, 상담사, 코치가 함께하며 정기적인 전문 커리큘럼
                            을 통해<br />검증된 정보와 실질적인 도움을 제공합니다.
                        </Text>
                        <Flex gap="20px">
                            <Center
                                bg="$primaryBgLight"
                                borderRadius="20px"
                                flex="1"
                                flexDir="column"
                                gap="10px"
                                overflow="hidden"
                                px="30px"
                                py="40px"
                            >
                                <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                                    전문 커리큘럼 운영
                                </Text>
                                <Text
                                    color="$text"
                                    textAlign="center"
                                    typography="bodyXlg"
                                    w="100%"
                                    wordBreak="keep-all"
                                >
                                    ADHD 전문가의 지속적인 양성과 체계적 관리 시스템
                                </Text>
                            </Center>
                            <Center
                                bg="$primaryBgLight"
                                borderRadius="20px"
                                flex="1"
                                flexDir="column"
                                gap="10px"
                                overflow="hidden"
                                px="30px"
                                py="40px"
                            >
                                <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                                    맞춤형 리포트
                                </Text>
                                <Text
                                    color="$text"
                                    textAlign="center"
                                    typography="bodyXlg"
                                    w="100%"
                                    wordBreak="keep-all"
                                >
                                    전문가가 구성한 알고리즘으로<br />나의 성향 완벽 분석
                                </Text>
                            </Center>
                            <Center
                                bg="$primaryBgLight"
                                borderRadius="20px"
                                flex="1"
                                flexDir="column"
                                gap="10px"
                                overflow="hidden"
                                px="30px"
                                py="40px"
                            >
                                <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                                    지속적인 코칭 및 관리
                                </Text>
                                <Text
                                    color="$text"
                                    textAlign="center"
                                    typography="bodyXlg"
                                    w="100%"
                                    wordBreak="keep-all"
                                >
                                    주기적으로 솔루션을 제시하고 <br />상담을 이어가는 관리 시스템
                                </Text>
                            </Center>
                        </Flex>
                    </VStack>
                </Flex>
            </Center>
            <Center
                display={[
                    "none",
                    null,
                    "flex",
                    null,
                    "none"
                ]}
                flexDir="column"
                overflow="hidden"
                px="30px"
                py="80px"
            >
                <VStack alignItems="center" gap="50px" maxW="1280px" w="100%">
                    <Text color="$title" typography="h4">
                        Solution
                    </Text>
                    <VStack gap="50px" w="100%">
                        <Image borderRadius="20px" h="300px" src="/images/Frame 1000014115.png" />
                        <Text color="$text" typography="bodyXlg" w="619px" wordBreak="keep-all">
                            <Text color="$primary" typography="title">
                                전문가와 함께 만드는, 신뢰 기반의 성인 ADHD 솔루션{" "}
                            </Text>
                            {" "}퍼즐핏은 단순한 검사 도구 제공이 아닌{" "}
                            연구 기반 자가검진, 맞춤 리포트, 코칭
                            을 통해 당사자가 사회 속에서 더 잘 기능하도록 돕는 통합 솔루션입니다.{"  "}
                            ADHD 전문 의사, 상담사, 코치가 함께하며 정기적인 전문 커리큘럼
                            을 통해<br />검증된 정보와 실질적인 도움을 제공합니다.
                        </Text>
                        <Flex gap="20px">
                            <Center
                                bg="$primaryBgLight"
                                borderRadius="20px"
                                flex="1"
                                flexDir="column"
                                gap="10px"
                                overflow="hidden"
                                px="30px"
                                py="40px"
                            >
                                <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                                    전문 커리큘럼 운영
                                </Text>
                                <Text
                                    color="$text"
                                    textAlign="center"
                                    typography="bodyXlg"
                                    w="100%"
                                    wordBreak="keep-all"
                                >
                                    ADHD 전문가의 지속적인 양성과 체계적 관리 시스템
                                </Text>
                            </Center>
                            <Center
                                bg="$primaryBgLight"
                                borderRadius="20px"
                                flex="1"
                                flexDir="column"
                                gap="10px"
                                overflow="hidden"
                                px="30px"
                                py="40px"
                            >
                                <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                                    맞춤형 리포트
                                </Text>
                                <Text
                                    color="$text"
                                    textAlign="center"
                                    typography="bodyXlg"
                                    w="100%"
                                    wordBreak="keep-all"
                                >
                                    전문가가 구성한 알고리즘으로<br />나의 성향 완벽 분석
                                </Text>
                            </Center>
                            <Center
                                bg="$primaryBgLight"
                                borderRadius="20px"
                                flex="1"
                                flexDir="column"
                                gap="10px"
                                overflow="hidden"
                                px="30px"
                                py="40px"
                            >
                                <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                                    지속적인 코칭 및 관리
                                </Text>
                                <Text
                                    color="$text"
                                    textAlign="center"
                                    typography="bodyXlg"
                                    w="100%"
                                    wordBreak="keep-all"
                                >
                                    주기적으로 솔루션을 제시하고 <br />상담을 이어가는 관리 시스템
                                </Text>
                            </Center>
                        </Flex>
                    </VStack>
                </VStack>
            </Center>
            <Center
                display={[
                    "flex",
                    null,
                    "none"
                ]}
                flexDir="column"
                overflow="hidden"
                pb="40px"
                pt="80px"
                px="20px"
            >
                <VStack alignItems="center" gap="50px" maxW="1280px" w="100%">
                    <Text color="$title" typography="h4">
                        Solution
                    </Text>
                    <VStack alignItems="center" gap="50px" w="100%">
                        <Image h="220px" src="/images/Frame 1000014115.png" w="360px" />
                        <Text
                            color="$text"
                            textAlign="center"
                            typography="bodyXlg"
                            w="100%"
                            wordBreak="keep-all"
                        >
                            <Text color="$primary" typography="bodyXlgBold">
                                전문가와 함께 만드는, <br />신뢰 기반의 성인 ADHD 솔루션{" "}
                            </Text>
                            {" "}퍼즐핏은 단순한 검사 도구 제공이 아닌{" "}
                            연구 기반 자가검진, 맞춤 리포트, 코칭
                            을 통해 당사자가 사회 속에서 더 잘 기능하도록 돕는 통합 솔루션입니다.{"  "}
                            ADHD 전문 의사, 상담사, 코치가 함께하며 정기적인 전문 커리큘럼
                            을 통해 검증된 정보와 실질적인 도움을 제공합니다.
                        </Text>
                        <VStack gap="20px" w="100%">
                            <Center
                                bg="$primaryBgLight"
                                borderRadius="12px"
                                flexDir="column"
                                gap="10px"
                                overflow="hidden"
                                px="30px"
                                py="40px"
                            >
                                <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                    전문 커리큘럼 운영
                                </Text>
                                <Text
                                    color="$text"
                                    textAlign="center"
                                    typography="bodyXlg"
                                    w="100%"
                                    wordBreak="keep-all"
                                >
                                    ADHD 전문가의 지속적인 양성과 체계적 관리 시스템
                                </Text>
                            </Center>
                            <Center
                                bg="$primaryBgLight"
                                borderRadius="12px"
                                flexDir="column"
                                gap="10px"
                                overflow="hidden"
                                px="30px"
                                py="40px"
                            >
                                <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                    맞춤형 리포트
                                </Text>
                                <Text
                                    color="$text"
                                    textAlign="center"
                                    typography="bodyXlg"
                                    w="100%"
                                    wordBreak="keep-all"
                                >
                                    전문가가 구성한 알고리즘으로<br />나의 성향 완벽 분석
                                </Text>
                            </Center>
                            <Center
                                bg="$primaryBgLight"
                                borderRadius="12px"
                                flexDir="column"
                                gap="10px"
                                overflow="hidden"
                                px="30px"
                                py="40px"
                            >
                                <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                    지속적인 코칭 및 관리
                                </Text>
                                <Text
                                    color="$text"
                                    textAlign="center"
                                    typography="bodyXlg"
                                    w="100%"
                                    wordBreak="keep-all"
                                >
                                    주기적으로 솔루션을 제시하고 <br />상담을 이어가는 관리 시스템
                                </Text>
                            </Center>
                        </VStack>
                    </VStack>
                </VStack>
            </Center>
            <Center
                display={[
                    "none",
                    null,
                    null,
                    null,
                    "flex"
                ]}
                flexDir="column"
                overflow="hidden"
                px="30px"
                py="100px"
            >
                <Center flexDir="column" gap="60px" maxW="1280px" w="100%">
                    <Center flexDir="column" gap="12px">
                        <Text color="$title" typography="h4">
                            Members
                        </Text>
                        <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                            퍼즐핏은 최고의 전문가들과 함께합니다.
                        </Text>
                    </Center>
                    <Flex gap="20px" w="100%">
                        <VStack flex="1">
                            <Box
                                bg="url(/icons/image.png) center/cover no-repeat, $gray200"
                                borderRadius="16px"
                                h="400px"
                                maxW="400px"
                                objectFit="cover"
                                overflow="hidden"
                                w="100%"
                            />
                            <VStack gap="10px" p="12px">
                                <Center gap="12px">
                                    <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                        맹소영{" "}
                                    </Text>
                                    <Box bg="$gray200" h="10px" w="2px" />
                                    <Text color="$primary" typography="bodyLgBold" wordBreak="keep-all">
                                        대표
                                    </Text>
                                </Center>
                                <VStack gap="4px">
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="29px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" flex="1" typography="bodyLg" wordBreak="keep-all">
                                            정신건강간호사 1급 (보건복지부)
                                        </Text>
                                    </Flex>
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="29px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" flex="1" typography="bodyLg" wordBreak="keep-all">
                                            보건교사 (교육부)
                                        </Text>
                                    </Flex>
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="29px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" flex="1" typography="bodyLg" wordBreak="keep-all">
                                            브레인바이오피드백트레이너 2급 (한국뇌파신경학회)
                                        </Text>
                                    </Flex>
                                </VStack>
                            </VStack>
                        </VStack>
                        <VStack flex="1">
                            <Box
                                bg="url(/icons/image.png) center/cover no-repeat, $gray200"
                                borderRadius="16px"
                                h="400px"
                                objectFit="cover"
                                overflow="hidden"
                            />
                            <VStack gap="10px" p="12px">
                                <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                    박혜연
                                </Text>
                                <VStack gap="4px">
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="29px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                                            정신건강임상심리사 1급 (보건복지부)
                                        </Text>
                                    </Flex>
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="29px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                                            임상심리전문가
                                        </Text>
                                    </Flex>
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="29px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" flex="1" typography="bodyLg" wordBreak="keep-all">
                                            브레인바이오피드백트레이너 2급<br />(한국뇌파신경학회)
                                        </Text>
                                    </Flex>
                                </VStack>
                            </VStack>
                        </VStack>
                        <VStack flex="1">
                            <Box
                                bg="url(/icons/image.png) center/cover no-repeat, $gray200"
                                borderRadius="16px"
                                h="400px"
                                objectFit="cover"
                                overflow="hidden"
                            />
                            <VStack gap="10px" p="12px">
                                <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                    김소연
                                </Text>
                                <VStack>
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="29px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                                            정신건강사회복지사 (보건복지부)
                                        </Text>
                                    </Flex>
                                </VStack>
                            </VStack>
                        </VStack>
                        <VStack flex="1">
                            <Box
                                bg="url(/icons/image.png) center/cover no-repeat, $gray200"
                                borderRadius="16px"
                                h="400px"
                                objectFit="cover"
                                overflow="hidden"
                            />
                            <VStack gap="10px" p="12px">
                                <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                    장우석
                                </Text>
                                <VStack>
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="29px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                                            풀스택 개발자
                                        </Text>
                                    </Flex>
                                </VStack>
                            </VStack>
                        </VStack>
                    </Flex>
                </Center>
            </Center>
            <Center
                display={[
                    "none",
                    null,
                    "flex",
                    null,
                    "none"
                ]}
                flexDir="column"
                overflow="hidden"
                px="30px"
                py="100px"
            >
                <Center flexDir="column" gap="60px" maxW="1280px" w="100%">
                    <Center flexDir="column" gap="12px">
                        <Text color="$title" typography="h4">
                            Members
                        </Text>
                        <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                            퍼즐핏은 최고의 전문가들과 함께합니다.
                        </Text>
                    </Center>
                    <VStack alignItems="center" gap="40px" w="100%">
                        <Flex alignItems="center" gap="20px" maxW="630px" w="100%">
                            <VStack flex="1">
                                <Box
                                    aspectRatio="0.76"
                                    bg="url(/icons/image.png) center/cover no-repeat, $gray200"
                                    borderRadius="16px"
                                    h="400px"
                                    maxH="524.59px"
                                    maxW="400px"
                                    objectFit="cover"
                                    overflow="hidden"
                                    w="100%"
                                />
                                <VStack gap="10px" p="12px">
                                    <Center gap="12px">
                                        <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                            맹소영{" "}
                                        </Text>
                                        <Box bg="$gray200" h="10px" w="2px" />
                                        <Text color="$primary" typography="bodyLgBold" wordBreak="keep-all">
                                            대표
                                        </Text>
                                    </Center>
                                    <VStack gap="4px">
                                        <Flex gap="8px">
                                            <Box
                                                bg="$gray300"
                                                h="29px"
                                                maskImage="url('/icons/Frame 287.svg')"
                                                maskPos="center"
                                                maskRepeat="no-repeat"
                                                maskSize="contain"
                                                w="6px"
                                            />
                                            <Text color="$text" flex="1" typography="bodyLg" wordBreak="keep-all">
                                                정신건강간호사 1급 (보건복지부)
                                            </Text>
                                        </Flex>
                                        <Flex gap="8px">
                                            <Box
                                                bg="$gray300"
                                                h="29px"
                                                maskImage="url('/icons/Frame 287.svg')"
                                                maskPos="center"
                                                maskRepeat="no-repeat"
                                                maskSize="contain"
                                                w="6px"
                                            />
                                            <Text color="$text" flex="1" typography="bodyLg" wordBreak="keep-all">
                                                보건교사 (교육부)
                                            </Text>
                                        </Flex>
                                        <Flex gap="8px">
                                            <Box
                                                bg="$gray300"
                                                h="29px"
                                                maskImage="url('/icons/Frame 287.svg')"
                                                maskPos="center"
                                                maskRepeat="no-repeat"
                                                maskSize="contain"
                                                w="6px"
                                            />
                                            <Text color="$text" flex="1" typography="bodyLg" wordBreak="keep-all">
                                                브레인바이오피드백트레이너 2급 (한국뇌파신경학회)
                                            </Text>
                                        </Flex>
                                    </VStack>
                                </VStack>
                            </VStack>
                            <VStack flex="1">
                                <Box
                                    aspectRatio="0.76"
                                    bg="url(/icons/image.png) center/cover no-repeat, $gray200"
                                    borderRadius="16px"
                                    h="400px"
                                    objectFit="cover"
                                    overflow="hidden"
                                />
                                <VStack gap="10px" p="12px">
                                    <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                        박혜연
                                    </Text>
                                    <VStack gap="4px">
                                        <Flex gap="8px">
                                            <Box
                                                bg="$gray300"
                                                h="29px"
                                                maskImage="url('/icons/Frame 287.svg')"
                                                maskPos="center"
                                                maskRepeat="no-repeat"
                                                maskSize="contain"
                                                w="6px"
                                            />
                                            <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                                                정신건강임상심리사 1급 (보건복지부)
                                            </Text>
                                        </Flex>
                                        <Flex gap="8px">
                                            <Box
                                                bg="$gray300"
                                                h="29px"
                                                maskImage="url('/icons/Frame 287.svg')"
                                                maskPos="center"
                                                maskRepeat="no-repeat"
                                                maskSize="contain"
                                                w="6px"
                                            />
                                            <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                                                임상심리전문가
                                            </Text>
                                        </Flex>
                                        <Flex gap="8px">
                                            <Box
                                                bg="$gray300"
                                                h="29px"
                                                maskImage="url('/icons/Frame 287.svg')"
                                                maskPos="center"
                                                maskRepeat="no-repeat"
                                                maskSize="contain"
                                                w="6px"
                                            />
                                            <Text color="$text" flex="1" typography="bodyLg" wordBreak="keep-all">
                                                브레인바이오피드백트레이너 2급<br />(한국뇌파신경학회)
                                            </Text>
                                        </Flex>
                                    </VStack>
                                </VStack>
                            </VStack>
                        </Flex>
                        <Flex alignItems="center" gap="20px" maxW="630px" w="100%">
                            <VStack flex="1">
                                <Box
                                    aspectRatio="0.76"
                                    bg="url(/icons/image.png) center/cover no-repeat, $gray200"
                                    borderRadius="16px"
                                    h="400px"
                                    objectFit="cover"
                                    overflow="hidden"
                                />
                                <VStack gap="10px" p="12px">
                                    <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                        김소연
                                    </Text>
                                    <VStack>
                                        <Flex gap="8px">
                                            <Box
                                                bg="$gray300"
                                                h="29px"
                                                maskImage="url('/icons/Frame 287.svg')"
                                                maskPos="center"
                                                maskRepeat="no-repeat"
                                                maskSize="contain"
                                                w="6px"
                                            />
                                            <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                                                정신건강사회복지사 (보건복지부)
                                            </Text>
                                        </Flex>
                                    </VStack>
                                </VStack>
                            </VStack>
                            <VStack flex="1">
                                <Box
                                    aspectRatio="0.76"
                                    bg="url(/icons/image.png) center/cover no-repeat, $gray200"
                                    borderRadius="16px"
                                    h="400px"
                                    objectFit="cover"
                                    overflow="hidden"
                                />
                                <VStack gap="10px" p="12px">
                                    <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                        장우석
                                    </Text>
                                    <VStack>
                                        <Flex gap="8px">
                                            <Box
                                                bg="$gray300"
                                                h="29px"
                                                maskImage="url('/icons/Frame 287.svg')"
                                                maskPos="center"
                                                maskRepeat="no-repeat"
                                                maskSize="contain"
                                                w="6px"
                                            />
                                            <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                                                풀스택 개발자
                                            </Text>
                                        </Flex>
                                    </VStack>
                                </VStack>
                            </VStack>
                        </Flex>
                    </VStack>
                </Center>
            </Center>
            <Center
                display={[
                    "flex",
                    null,
                    "none"
                ]}
                flexDir="column"
                overflow="hidden"
                pb="80px"
                pt="40px"
                px="20px"
            >
                <Center flexDir="column" gap="40px" maxW="1000px" w="100%">
                    <Center flexDir="column" gap="12px">
                        <Text color="$title" typography="h4">
                            Members
                        </Text>
                        <Text color="$text" typography="bodyXlg" wordBreak="keep-all">
                            퍼즐핏은 최고의 전문가들과 함께합니다.
                        </Text>
                    </Center>
                    <VStack gap="40px" px="10px" w="100%">
                        <VStack>
                            <Box
                                bg="url(/icons/image.png) center/cover no-repeat, $gray200"
                                borderRadius="8px"
                                h="400px"
                                maxW="400px"
                                objectFit="cover"
                                overflow="hidden"
                                w="100%"
                            />
                            <VStack gap="12px" p="12px">
                                <Center gap="12px">
                                    <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                        맹소영{" "}
                                    </Text>
                                    <Box bg="$gray200" h="10px" w="2px" />
                                    <Text color="$primary" typography="bodyXlgBold" wordBreak="keep-all">
                                        대표
                                    </Text>
                                </Center>
                                <VStack gap="4px">
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="26px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" flex="1" typography="bodyLg" wordBreak="keep-all">
                                            정신건강간호사 1급 (보건복지부)
                                        </Text>
                                    </Flex>
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="26px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" flex="1" typography="bodyLg" wordBreak="keep-all">
                                            보건교사 (교육부)
                                        </Text>
                                    </Flex>
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="26px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" flex="1" typography="bodyLg" wordBreak="keep-all">
                                            브레인바이오피드백트레이너 2급 <br />(한국뇌파신경학회)
                                        </Text>
                                    </Flex>
                                </VStack>
                            </VStack>
                        </VStack>
                        <VStack>
                            <Box bg="url(/icons/image.png) center/cover no-repeat, $gray200" borderRadius="8px" h="400px" overflow="hidden" />
                            <VStack gap="10px" p="12px">
                                <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                    박혜연
                                </Text>
                                <VStack gap="4px">
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="26px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                                            정신건강임상심리사 1급 (보건복지부)
                                        </Text>
                                    </Flex>
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="26px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                                            임상심리전문가
                                        </Text>
                                    </Flex>
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="26px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" flex="1" typography="bodyLg" wordBreak="keep-all">
                                            브레인바이오피드백트레이너 2급<br />(한국뇌파신경학회)
                                        </Text>
                                    </Flex>
                                </VStack>
                            </VStack>
                        </VStack>
                        <VStack>
                            <Box
                                bg="url(/icons/image.png) center/cover no-repeat, $gray200"
                                borderRadius="8px"
                                h="400px"
                                objectFit="cover"
                                overflow="hidden"
                            />
                            <VStack gap="10px" p="12px">
                                <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                    김소연
                                </Text>
                                <VStack>
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="26px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                                            정신건강사회복지사 (보건복지부)
                                        </Text>
                                    </Flex>
                                </VStack>
                            </VStack>
                        </VStack>
                        <VStack>
                            <Box
                                bg="url(/icons/image.png) center/cover no-repeat, $gray200"
                                borderRadius="8px"
                                h="400px"
                                objectFit="cover"
                                overflow="hidden"
                            />
                            <VStack gap="10px" p="12px">
                                <Text color="$text" typography="bodyXlgBold" wordBreak="keep-all">
                                    장우석
                                </Text>
                                <VStack>
                                    <Flex gap="8px">
                                        <Box
                                            bg="$gray300"
                                            h="26px"
                                            maskImage="url('/icons/Frame 287.svg')"
                                            maskPos="center"
                                            maskRepeat="no-repeat"
                                            maskSize="contain"
                                            w="6px"
                                        />
                                        <Text color="$text" typography="bodyLg" wordBreak="keep-all">
                                            풀스택 개발자
                                        </Text>
                                    </Flex>
                                </VStack>
                            </VStack>
                        </VStack>
                    </VStack>
                </Center>
            </Center>
            <Footer status="landing" />
        </VStack>
    )
}
