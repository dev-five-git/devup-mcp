export interface ButtonProps {
    leftIcon?: boolean
    rightIcon?: boolean
    size: 'lg' | 'md' | 'sm' | 'tag'
    varient: 'primary' | 'white' | 'ghost' | 'disabled' | 'error'
}

export function Button({ leftIcon, rightIcon, size, varient }: ButtonProps) {
    return (
        <Center
            _active={{
                "bg": {
                    primary: "$primaryDarkest",
                    disabled: "$errorDarkest",
                    white: "$gray200",
                    ghost: "$gray200",
                    error: "$errorDarkest"
                }[varient],
                "borderRadius": varient === 'disabled' && "6px",
                "px": varient === 'disabled' && "10px"
            }}
            _hover={{
                "bg": {
                    primary: "$primaryDark",
                    disabled: "$errorDark",
                    white: "$gray100",
                    ghost: "$gray100",
                    error: "$errorDark"
                }[varient],
                "borderRadius": varient === 'disabled' && "6px",
                "px": varient === 'disabled' && "10px",
                "gap": size === 'md' && "10px"
            }}
            bg={{
                primary: "$primary",
                disabled: "$gray200",
                white: "$innerBg",
                error: "$error"
            }[varient]}
            border={varient === 'white' && "solid 1px $border"}
            borderRadius={{
                lg: "8px",
                md: "8px",
                sm: "8px",
                tag: "6px"
            }[size]}
            boxShadow={{
                primary: "0 1px 2px -1px #0000001A, 0 1px 3px 0 #0000001A",
                error: "0 1px 2px -1px #0000001A, 0 1px 3px 0 #0000001A"
            }[varient]}
            gap={{
                lg: "10px",
                md: {
                    primary: "10px",
                    disabled: "10px",
                    white: "10px",
                    ghost: "8px",
                    error: "10px"
                }[varient],
                sm: "8px"
            }[size]}
            px={{
                lg: {
                    primary: "24px",
                    disabled: "24px",
                    white: "24px",
                    ghost: "10px",
                    error: "24px"
                }[varient],
                md: {
                    primary: "16px",
                    disabled: "16px",
                    white: "16px",
                    ghost: "12px",
                    error: "16px"
                }[varient],
                sm: {
                    primary: "12px",
                    disabled: "12px",
                    white: "12px",
                    ghost: "10px",
                    error: "12px"
                }[varient],
                tag: "10px"
            }[size]}
        >
            {leftIcon && (size === "lg" || size === "md" || size === "sm") && (
                <Box
                    aspectRatio="1"
                    bg="$gray400"
                    boxSize={{
                        lg: {
                            primary: "20px",
                            disabled: "20px",
                            white: "20px",
                            ghost: "16px",
                            error: "20px"
                        }[varient],
                        md: "16px",
                        sm: "12px"
                    }[size]}
                    maskImage="url('/icons/속성 1=user.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
            <Text
                color={{
                    primary: "#FFF",
                    disabled: "$gray400",
                    white: "$text",
                    ghost: "$text",
                    error: "#FFF"
                }[varient]}
                typography={({
                    lg: "buttonLg",
                    md: "button",
                    sm: "buttonSm",
                    tag: "tag"
                } as const)[size]}
            >
                {{
                    lg: "buttonLg",
                    md: "button",
                    sm: "button",
                    tag: "Tag"
                }[size]}
            </Text>
            {rightIcon && (size === "lg" || size === "md" || size === "sm") && (
                <Box
                    bg="$gray400"
                    boxSize={{
                        lg: {
                            primary: "20px",
                            disabled: "20px",
                            white: "20px",
                            ghost: "18px",
                            error: "20px"
                        }[varient],
                        md: "16px",
                        sm: "14px"
                    }[size]}
                    maskImage="url('/icons/속성 1=right.svg')"
                    maskPos="center"
                    maskRepeat="no-repeat"
                    maskSize="contain"
                />
            )}
        </Center>
    )
}
