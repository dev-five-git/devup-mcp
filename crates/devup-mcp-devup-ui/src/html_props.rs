//! Generated from @types/react 19.2.17 JSX.IntrinsicElements via TypeScript's type checker.
//! Regenerate with tools/generate-html-props.cjs; do not add guessed attributes.
//! @devup-ui/react 1.0.41 merges ComponentProps<T> for literal intrinsic as=T.

pub(crate) fn is_html_prop(tag: &str, prop: &str) -> bool {
    let specific = match tag {
        "a" => "download href hrefLang media ping referrerPolicy target type",
        "abbr" | "address" | "article" | "aside" | "b" | "bdi" | "bdo" | "big" | "body" | "br"
        | "caption" | "center" | "cite" | "code" | "datalist" | "dd" | "dfn" | "div" | "dl"
        | "dt" | "em" | "figcaption" | "figure" | "footer" | "h1" | "h2" | "h3" | "h4" | "h5"
        | "h6" | "head" | "header" | "hgroup" | "hr" | "i" | "kbd" | "legend" | "main" | "mark"
        | "menuitem" | "nav" | "noindex" | "noscript" | "p" | "picture" | "pre" | "rp" | "rt"
        | "ruby" | "s" | "samp" | "search" | "section" | "small" | "span" | "strong" | "sub"
        | "summary" | "sup" | "template" | "tbody" | "tfoot" | "thead" | "title" | "tr" | "u"
        | "ul" | "var" | "wbr" => "",
        "area" => "alt coords download href hrefLang media referrerPolicy shape target",
        "audio" => {
            "autoPlay controls controlsList crossOrigin loop mediaGroup muted playsInline preload src"
        }
        "base" => "href target",
        "blockquote" | "q" => "cite",
        "button" => {
            "disabled form formAction formEncType formMethod formNoValidate formTarget name type value"
        }
        "canvas" => "height width",
        "col" => "span width",
        "colgroup" => "span",
        "data" | "li" => "value",
        "del" | "ins" => "cite dateTime",
        "details" => "name open",
        "dialog" => "closedby open",
        "embed" => "height src type width",
        "fieldset" => "disabled form name",
        "form" => "acceptCharset action autoComplete encType method name noValidate target",
        "html" => "manifest",
        "iframe" => {
            "allow allowFullScreen allowTransparency frameBorder height loading marginHeight marginWidth name referrerPolicy sandbox scrolling seamless src srcDoc width"
        }
        "img" => {
            "alt crossOrigin decoding fetchPriority height loading referrerPolicy sizes src srcSet useMap width"
        }
        "input" => {
            "accept alt autoComplete capture checked disabled form formAction formEncType formMethod formNoValidate formTarget height list max maxLength min minLength multiple name pattern placeholder readOnly required size src step type value width"
        }
        "keygen" => "challenge disabled form keyParams keyType name",
        "label" => "form htmlFor",
        "link" => {
            "as blocking charSet crossOrigin fetchPriority href hrefLang imageSizes imageSrcSet integrity media precedence referrerPolicy sizes type"
        }
        "map" | "slot" => "name",
        "menu" => "type",
        "meta" => "charSet httpEquiv media name",
        "meter" => "form high low max min optimum value",
        "object" => "classID data form height name type useMap width wmode",
        "ol" => "reversed start type",
        "optgroup" => "disabled label",
        "option" => "disabled label selected value",
        "output" => "form htmlFor name",
        "param" => "name value",
        "progress" => "max value",
        "script" => {
            "async blocking charSet crossOrigin defer fetchPriority integrity noModule referrerPolicy src type"
        }
        "select" => "autoComplete disabled form multiple name required size value",
        "source" => "height media sizes src srcSet type width",
        "style" => "blocking href media precedence scoped type",
        "table" => "align bgcolor border cellPadding cellSpacing frame rules summary width",
        "td" => "abbr align colSpan headers height rowSpan scope valign width",
        "textarea" => {
            "autoComplete cols dirName disabled form maxLength minLength name placeholder readOnly required rows value wrap"
        }
        "th" => "abbr align colSpan headers rowSpan scope",
        "time" => "dateTime",
        "track" => "default kind label src srcLang",
        "video" => {
            "autoPlay controls controlsList crossOrigin disablePictureInPicture disableRemotePlayback height loop mediaGroup muted playsInline poster preload src width"
        }
        "webview" => {
            "allowFullScreen allowpopups autosize blinkfeatures disableblinkfeatures disableguestresize disablewebsecurity guestinstance httpreferrer nodeintegration partition plugins preload src useragent webpreferences"
        }
        _ => return false,
    };
    "about accessKey autoCapitalize autoCorrect autoFocus autoSave children className color content contentEditable contextMenu dangerouslySetInnerHTML datatype defaultChecked defaultValue dir draggable enterKeyHint exportparts hidden id inert inlist inputMode is itemID itemProp itemRef itemScope itemType key lang nonce part popover popoverTarget popoverTargetAction prefix property radioGroup ref rel resource results rev role security slot spellCheck style suppressContentEditableWarning suppressHydrationWarning tabIndex title translate typeof unselectable vocab".split_whitespace().chain(specific.split_whitespace()).any(|known| known == prop)
}
