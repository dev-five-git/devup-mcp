/// Browser contracts for UI regressions that a settled screenshot cannot prove.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RegressionInput {
    StickyHeader {
        header: String,
        hero: String,
        logo: String,
        links: String,
        scrolled_background: String,
    },
    Inquiry {
        open: String,
        detail: String,
        description: String,
        description_text: String,
        inquire: String,
        form: String,
        choice: InquiryChoice,
        fields: Vec<InquiryField>,
        submit: String,
        error: String,
        recipient: String,
    },
    ImageHandoff {
        /// An application selector attached only once the opening action is hydrated.
        ready: Option<String>,
        open: Option<String>,
        images: String,
        next: Option<String>,
        /// Swipe the current image left or right instead of clicking next.
        #[schemars(extend("enum" = ["left", "right"]))]
        swipe: Option<String>,
        next_src: String,
    },
    FontStability {
        fonts: Vec<String>,
        targets: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InquiryChoice {
    pub control: String,
    pub value: String,
    /// Custom listboxes name both the option to click and the selected label.
    /// Omit both for a native select; value then names the option value.
    pub option: Option<String>,
    pub selected: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InquiryField {
    pub selector: String,
    pub value: String,
}

fn validate_regression(config: &RegressionInput) -> Result<(), DevupError> {
    let text = |name, value: &str| bounded_text(name, value);
    let selector = |value: &str| {
        text("regression selector", value)?;
        if value.contains("data-figma") || value.contains("data-source-node") {
            return Err(invalid("Regression checks require application selectors."));
        }
        Ok(())
    };
    match config {
        RegressionInput::StickyHeader {
            header,
            hero,
            logo,
            links,
            scrolled_background,
        } => {
            for value in [header, hero, logo, links] {
                selector(value)?;
            }
            text("regression.scrolledBackground", scrolled_background)?;
            if !scrolled_background.starts_with("rgb(") {
                return Err(invalid(
                    "scrolledBackground requires an opaque computed rgb() color.",
                ));
            }
        }
        RegressionInput::Inquiry {
            open,
            detail,
            description,
            description_text,
            inquire,
            form,
            choice,
            fields,
            submit,
            error,
            recipient,
        } => {
            for value in [
                open,
                detail,
                description,
                inquire,
                form,
                &choice.control,
                submit,
                error,
            ] {
                selector(value)?;
            }
            for value in [&choice.option, &choice.selected].into_iter().flatten() {
                selector(value)?;
            }
            if (choice.option.is_some() && choice.selected.is_none())
                || !(1..=8).contains(&fields.len())
            {
                return Err(invalid(
                    "Inquiry needs 1-8 fields and, for a custom select, both option and selected selectors.",
                ));
            }
            text("regression.descriptionText", description_text)?;
            text("regression.choice.value", &choice.value)?;
            text("regression.recipient", recipient)?;
            for field in fields {
                selector(&field.selector)?;
                text("regression field value", &field.value)?;
            }
        }
        RegressionInput::ImageHandoff {
            ready,
            open,
            images,
            next,
            swipe,
            next_src,
        } => {
            if next.is_some() == swipe.is_some()
                || swipe
                    .as_deref()
                    .is_some_and(|direction| !matches!(direction, "left" | "right"))
            {
                return Err(invalid(
                    "Image handoff requires exactly one of next or swipe (left/right).",
                ));
            }
            for value in [ready, open, next].into_iter().flatten() {
                selector(value)?;
            }
            selector(images)?;
            validate_route("regression.nextSrc", next_src)?;
        }
        RegressionInput::FontStability { fonts, targets } => {
            if !(1..=8).contains(&fonts.len()) || !(1..=16).contains(&targets.len()) {
                return Err(invalid(
                    "Font stability needs 1-8 font paths and 1-16 first-viewport selectors.",
                ));
            }
            for path in fonts {
                validate_route("regression font path", path)?;
            }
            for value in targets {
                selector(value)?;
            }
        }
    }
    Ok(())
}
