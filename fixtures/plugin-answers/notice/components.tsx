export interface HeaderItemProps {
    property1: '기본' | 'selected'
}

export function HeaderItem({ property1 }: HeaderItemProps) {
    return (
        <Center
            borderBottom={property1 === 'selected' && "solid 3px $text"}
            py="8px"
        >
            <Text color="$text" typography="headerMenu">
                menu
            </Text>
        </Center>
    )
}

export function LanguageButton() {
    return (
        <Flex
            _active={{
                "bg": "#B5B5B580",
                "bgBlendMode": null
            }}
            _hover={{
                "bg": "#B5B5B540",
                "bgBlendMode": null
            }}
            alignItems="center"
            borderRadius="100px"
            gap="8px"
            px="12px"
            py="8px"
        >
            <Box
                aspectRatio="1"
                bg="$text"
                boxSize="24px"
                maskImage="url(/icons/grommet-icons:language.svg)"
                maskPos="center"
                maskRepeat="no-repeat"
                maskSize="contain"
            />
            <Text color="$text" typography="headerMenuSm">
                KOR
            </Text>
            <Box
                aspectRatio="1"
                bg="$text"
                boxSize="16px"
                maskImage="url(/icons/grommet-icons:language.svg)"
                maskPos="center"
                maskRepeat="no-repeat"
                maskSize="contain"
            />
        </Flex>
    )
}

export interface HeaderProps {
    property1: 'scroll' | 'transparent' | 'mobileTranspa' | 'mobileScroll'
}

export function Header({ property1 }: HeaderProps) {
    return (
        <Flex
            alignItems="center"
            backdropFilter={{
                scroll: "blur(20px)",
                mobileScroll: "blur(10px)"
            }[property1]}
            bg={{
                scroll: "$headerBg",
                mobileScroll: "$headerBg"
            }[property1]}
            boxShadow={{
                scroll: "0 0 15px 0 #0000000D",
                mobileScroll: "0 4px 10px 0 #0000000D"
            }[property1]}
            justifyContent="space-between"
            overflow="hidden"
            px={{
                scroll: "20px",
                transparent: "20px",
                mobileTranspa: "16px",
                mobileScroll: "16px"
            }[property1]}
        >
            {property1 === "scroll" && <Image flex="1" maxW="1640px" src="/icons/Frame 1000014227.svg" w="100%" />}
            {property1 === "transparent" && (
                <Flex
                    alignItems="center"
                    flex="1"
                    justifyContent="space-between"
                    maxW="1640px"
                    w="100%"
                >
                    <Flex alignItems="center" gap="30px">
                        {/* <Logo /> */}
                        <Box
                            bg="$text"
                            h="20px"
                            maskImage="url(/icons/Logo.svg)"
                            maskPos="center"
                            maskRepeat="no-repeat"
                            maskSize="contain"
                            w="280px"
                        />
                        <Flex alignItems="center" gap="30px">
                            <Box bg="#FFF" h="24px" opacity="0.2" w="2px" />
                            <HeaderItem property1="기본" />
                            <HeaderItem property1="기본" />
                            <HeaderItem property1="기본" />
                            <HeaderItem property1="기본" />
                        </Flex>
                    </Flex>
                    <LanguageButton />
                </Flex>
            )}
            {(property1 === "mobileTranspa" || property1 === "mobileScroll") && (
                <Flex
                    alignItems="center"
                    flex="1"
                    justifyContent="space-between"
                    maxW="1640px"
                    w="100%"
                >
                    <Box
                        bg="$text"
                        h="14px"
                        maskImage="url(/icons/Logo.svg)"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                        w="196px"
                    />
                    <Box
                        bg={{
                            mobileTranspa: "#FFF",
                            mobileScroll: "$text"
                        }[property1]}
                        boxSize="24px"
                        maskImage="url(/icons/Header.svg)"
                        maskPos="center"
                        maskRepeat="no-repeat"
                        maskSize="contain"
                    />
                </Flex>
            )}
        </Flex>
    )
}

export interface TabProps {
    /** text */
    children: React.ReactNode
}

export function Tab({ children }: TabProps) {
    return (
        <Flex
            _selected={{
                "gap": "4px"
            }}
            gap="4px"
            justifyContent="center"
        >
            <Text color="$text" typography="noticeSelected" wordBreak="keep-all">
                {children}
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
    )
}

export interface IconsProps {
    Property1: 'plus' | 'minus' | 'write' | 'left' | 'right' | 'bottom' | 'top' | 'leftSkip' | 'rightSkip' | 'globe' | 'language' | 'drag' | 'search' | 'delete' | 'document' | 'caution' | 'folder' | 'import' | 'folderPlus' | 'hamburger' | 'close' | 'imagePlus' | 'image' | 'export' | 'addCircle' | 'meatball' | 'code' | 'info' | 'public' | 'lock' | 'unlock' | 'comment' | 'comment-fill' | 'copy' | 'duplicate' | 'paste' | 'cut' | 'folderMinus' | 'publicOutline' | 'privacy' | 'Security' | 'print' | 'newDocu' | 'save' | 'sort' | 'history' | 'clip' | 'upload' | 'desktop' | 'mobile' | 'home' | 'download' | 'page' | 'closedEye' | 'setting' | 'setting-fill'
}

export function Icons({ Property1 }: IconsProps) {
    return (
        <>
            {Property1 === "plus" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=plus.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "minus" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=minus.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "left" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=left.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "right" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=right.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "bottom" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=bottom.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "top" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=top.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "leftSkip" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=leftSkip.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "rightSkip" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=rightSkip.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "write" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=write.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "globe" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=globe.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "language" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=language.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "drag" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=drag.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "search" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=search.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "delete" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=delete.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "page" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=page.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "document" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=document.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "newDocu" && <Image src="/icons/Property 1=newDocu.svg" />}
            {Property1 === "caution" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=caution.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "folder" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=folder.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "folderPlus" && <Image src="/icons/Property 1=folderPlus.svg" />}
            {Property1 === "folderMinus" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=folderMinus.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "import" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=import.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "export" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=export.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "image" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=image.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "imagePlus" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=imagePlus.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "hamburger" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=hamburger.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "close" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=close.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "addCircle" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=addCircle.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "meatball" && <Image src="/icons/Property 1=meatball.svg" />}
            {Property1 === "code" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=code.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "info" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=info.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "public" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=public.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "publicOutline" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=publicOutline.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "lock" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=lock.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "unlock" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=unlock.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "comment" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=comment.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "comment-fill" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=comment-fill.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "copy" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=copy.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "duplicate" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=duplicate.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "paste" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=paste.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "cut" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=cut.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "privacy" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=privacy.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "Security" && <Image src="/icons/Property 1=Security.svg" />}
            {Property1 === "closedEye" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=closedEye.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "print" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=print.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "save" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=save.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "history" && <Image src="/icons/Property 1=history.svg" />}
            {Property1 === "sort" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=sort.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "clip" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=clip.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "upload" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=upload.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "desktop" && <Image src="/icons/Property 1=desktop.svg" />}
            {Property1 === "mobile" && <Image src="/icons/Property 1=mobile.svg" />}
            {Property1 === "home" && (
                <Box
                    bg="#000"
                    maskImage="url('/icons/Property 1=home.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "download" && (
                <Box
                    bg="#000"
                    maskImage="url('/icons/Property 1=download.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "setting" && (
                <Box
                    bg="#000"
                    maskImage="url('/icons/Property 1=setting.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            {Property1 === "setting-fill" && (
                <Box
                    bg="$text"
                    maskImage="url('/icons/Property 1=setting-fill.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
        </>
    )
}

export function Pagination() {
    return (
        <Center
            _active={{
                "border": "solid 1px $primary",
                "bg": "$primaryBg"
            }}
            _hover={{
                "border": "solid 1px $primary"
            }}
            _selected={{
                "border": "solid 2px $primary"
            }}
            aspectRatio="1"
            bg="$innerBg"
            border="solid 2px $primary"
            borderRadius="1000px"
            flexDir="column"
        >
            <Text color="$primary" typography="pagination">
                1
            </Text>
        </Center>
    )
}

export interface ThemeButtonProps {
    property1: 'Dark' | 'Light'
}

export function ThemeButton({ property1 }: ThemeButtonProps) {
    return (
        <Flex
            alignItems="center"
            bg="#FFF"
            borderRadius="1000px"
            justifyContent={{
                Light: "flex-start",
                Dark: "flex-end"
            }[property1]}
            overflow="hidden"
            p="2px"
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
                    maskImage={{
                        Light: "url(/icons/light.svg)",
                        Dark: "url(/icons/dark.svg)"
                    }[property1]}
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            </Flex>
        </Flex>
    )
}

export interface FooterProps {
    property1: 'desktop' | 'mobile' | 'tablet'
}

export function Footer({ property1 }: FooterProps) {
    return (
        <Flex
            bg="#2D2926"
            justifyContent="center"
            overflow="hidden"
            p={property1 === 'desktop' && "60px"}
            px={{
                tablet: "30px",
                mobile: "16px"
            }[property1]}
            py={{
                tablet: "40px",
                mobile: "40px"
            }[property1]}
        >
            {property1 === "desktop" && (
                <Flex flex="1" gap="80px" maxW="1280px" w="100%">
                    <Flex gap="80px">
                        <VStack gap="20px">
                            {/* <Logo /> */}
                            <Box
                                bg="$text"
                                h="16px"
                                maskImage="url(/icons/Logo.svg)"
                                maskPos="center"
                                maskRepeat="no-repeat"
                                maskSize="contain"
                                w="220px"
                            />
                            <Flex gap="16px">
                                <ThemeButton property1="Light" />
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
            )}
            {property1 === "tablet" && (
                <VStack flex="1" gap="30px" maxW="1280px" w="100%">
                    <Flex alignItems="center" gap="30px" justifyContent="flex-end">
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
                    <Flex gap="80px">
                        <Box
                            bg="#FFF"
                            maskImage="url('/icons/Frame 1000014177.svg')"
                            maskPos="center"
                            maskRepeat="no-repeat"
                            maskSize="contain"
                        />
                        <VStack gap="10px">
                            <Text color="#FFF" typography="footerTitle" wordBreak="keep-all">
                                라온피플(주)
                            </Text>
                            <Text color="#FFF" opacity="0.5" typography="footerText" wordBreak="keep-all">
                                대표이사 : 이석중 주소 : 13840 경기 과천시 과천대로7나길 60 과천어반허브, C동 5층/6층<br />TEL : 1899-3058<br />FAX : 02-3318-3351<br />이메일 : sales@laonpeople.com{" "}
                            </Text>
                        </VStack>
                    </Flex>
                </VStack>
            )}
            {property1 === "mobile" && (
                <VStack
                    alignItems="center"
                    flex="1"
                    gap="40px"
                    maxW="1280px"
                    w="100%"
                >
                    <Flex gap="30px" justifyContent="center" w="100%">
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
                    <VStack alignItems="center" gap="40px" w="100%">
                        <Center flexDir="column" gap="20px">
                            {/* <Logo /> */}
                            <Box
                                bg="$text"
                                h="16px"
                                maskImage="url(/icons/Logo.svg)"
                                maskPos="center"
                                maskRepeat="no-repeat"
                                maskSize="contain"
                                w="220px"
                            />
                            <Flex gap="16px">
                                <ThemeButton property1="Light" />
                                <Box
                                    bg="#FFF"
                                    h="32px"
                                    maskImage="url('/icons/Frame 1000014232.svg')"
                                    maskPos="center"
                                    maskRepeat="no-repeat"
                                    maskSize="contain"
                                />
                            </Flex>
                        </Center>
                        <VStack alignItems="center" gap="10px" w="100%">
                            <Text color="#FFF" typography="footerTitle" wordBreak="keep-all">
                                라온피플(주)
                            </Text>
                            <Text
                                color="#FFF"
                                opacity="0.5"
                                textAlign="center"
                                typography="footerText"
                                w="100%"
                                wordBreak="keep-all"
                            >
                                대표이사 : 이석중 주소 : 13840 경기 과천시 과천대로7나길 60 과천어반허브, <br />C동 5층/6층<br />TEL : 1899-3058<br />FAX : 02-3318-3351<br />이메일 : sales@laonpeople.com{" "}
                            </Text>
                        </VStack>
                    </VStack>
                </VStack>
            )}
        </Flex>
    )
}
