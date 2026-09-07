use devup_mcp_devup_ui::theme::{Completeness, ThemeScope, VariableSnapshot, generate_devup_json};
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn maps_variables_modes_aliases_and_styles_to_devup_json() {
    let variables: VariableSnapshot = serde_json::from_value(json!({
        "collections": [{
            "id": "collection:1", "name": "Foundation", "defaultModeId": "mode:light",
            "modes": [
                {"modeId": "mode:light", "name": "Light"},
                {"modeId": "mode:dark", "name": "Dark Mode"}
            ]
        }],
        "variables": [
            {
                "id": "var:primary", "name": "Color/Primary", "resolvedType": "COLOR",
                "variableCollectionId": "collection:1", "codeSyntax": {"WEB": "primary"},
                "valuesByMode": {
                    "mode:light": {"r": 0, "g": 0.4392156863, "b": 0.9529411765, "a": 1},
                    "mode:dark": {"r": 0.1960784314, "g": 0.568627451, "b": 1, "a": 1}
                }
            },
            {
                "id": "var:accent", "name": "Color/Accent", "resolvedType": "COLOR",
                "variableCollectionId": "collection:1", "codeSyntax": {},
                "valuesByMode": {
                    "mode:light": {"type": "VARIABLE_ALIAS", "id": "var:primary"},
                    "mode:dark": {"type": "VARIABLE_ALIAS", "id": "var:primary"}
                }
            },
            {
                "id": "var:gutter", "name": "Spacing/Gutter", "resolvedType": "FLOAT",
                "variableCollectionId": "collection:1", "codeSyntax": {"WEB": "gutter"},
                "valuesByMode": {"mode:light": 16, "mode:dark": 20}
            }
        ],
        // A style's value is what the snapshot script serialises: the text
        // style itself, and an effect style's `effects`.
        "styles": [
            {"id": "style:text", "name": "Heading/H1", "styleType": "TEXT", "value": {
                "fontName": {"family": "Pretendard", "style": "Bold"}, "fontSize": 32,
                "lineHeight": {"unit": "PERCENT", "value": 130.00000476837158},
                "letterSpacing": {"unit": "PERCENT", "value": -2},
                "textCase": "ORIGINAL", "textDecoration": "NONE", "boundVariables": {}
            }},
            {"id": "style:effect", "name": "Elevation/Card", "styleType": "EFFECT", "value": [
                {"type": "DROP_SHADOW", "visible": true, "offset": {"x": 0, "y": 4}, "radius": 12, "spread": 0,
                 "color": {"r": 0, "g": 0, "b": 0, "a": 0.1}, "blendMode": "NORMAL"}
            ]}
        ],
        "usedRemoteVariables": [],
        "localComplete": true,
        "usedRemoteComplete": true
    }))
    .expect("variable snapshot");

    let output = generate_devup_json(&variables, ThemeScope::File).expect("devup theme");
    assert_eq!(output.completeness, Completeness::FullLocalPlusUsedRemote);
    // The collection's default mode is written first: devup-ui takes the
    // first theme under `colors` as the one in effect without a `data-theme`,
    // so `light` before `darkMode` here is what makes a Light-by-default file
    // light by default. Sorted by name it was the other way round.
    assert_eq!(
        output.json,
        concat!(
            "{\n",
            "  \"theme\": {\n",
            "    \"colors\": {\n",
            "      \"light\": {\n",
            "        \"accent\": \"#0070f3\",\n",
            "        \"primary\": \"#0070f3\"\n",
            "      },\n",
            "      \"darkMode\": {\n",
            "        \"accent\": \"#3291ff\",\n",
            "        \"primary\": \"#3291ff\"\n",
            "      }\n",
            "    },\n",
            // In the shape devup-ui reads: pixels, a line-height ratio, an em
            // of letter-spacing, and the weight the font style's name means.
            "    \"typography\": {\n",
            "      \"headingH1\": {\n",
            "        \"fontFamily\": \"Pretendard\",\n",
            "        \"fontWeight\": 700,\n",
            "        \"fontSize\": \"32px\",\n",
            "        \"lineHeight\": 1.3,\n",
            "        \"letterSpacing\": \"-0.02em\"\n",
            "      }\n",
            "    },\n",
            "    \"length\": {\n",
            "      \"light\": {\n",
            "        \"gutter\": \"16px\"\n",
            "      },\n",
            "      \"darkMode\": {\n",
            "        \"gutter\": \"20px\"\n",
            "      }\n",
            "    },\n",
            // A shadow is the same at every colour theme, and devup-ui keys it
            // by one, so each theme gets it.
            "    \"shadow\": {\n",
            "      \"light\": {\n",
            "        \"elevationCard\": \"0 4px 12px 0 #0000001a\"\n",
            "      },\n",
            "      \"darkMode\": {\n",
            "        \"elevationCard\": \"0 4px 12px 0 #0000001a\"\n",
            "      }\n",
            "    }\n",
            "  }\n",
            "}\n"
        )
    );
    assert_eq!(output.counts.variables, 3);
    assert_eq!(output.counts.styles, 2);
}

/// Styles named for a breakpoint are one entry with a slot each: the
/// plugin's `styleNameToTypography` puts `mobile/`, `tablet/` and `desktop/`
/// at slots 0, 2 and 4 and a leading number at that slot, and the slots are
/// gathered into a responsive array with the empty tail dropped. A text
/// style that sets its line height automatically is `normal`, one in pixels
/// stays in pixels, `Bold Italic` is bold and italic, and a bound font size
/// is its variable's token.
#[test]
fn styles_named_for_a_breakpoint_become_one_responsive_entry() {
    let variables: VariableSnapshot = serde_json::from_value(json!({
        "collections": [{"id": "c", "name": "C", "defaultModeId": "m", "modes": [{"modeId": "m", "name": "Default"}]}],
        "variables": [{
            "id": "var:size", "name": "Font/Size Lg", "resolvedType": "FLOAT",
            "variableCollectionId": "c", "codeSyntax": {}, "valuesByMode": {"m": 40}
        }],
        "styles": [
            {"id": "s:0", "name": "mobile/h1", "styleType": "TEXT", "value": {
                "fontName": {"family": "Pretendard", "style": "Bold Italic"}, "fontSize": 24,
                "lineHeight": {"unit": "AUTO"}, "letterSpacing": {"unit": "PIXELS", "value": 0.5},
                "textCase": "UPPER", "textDecoration": "UNDERLINE", "boundVariables": {}
            }},
            {"id": "s:4", "name": "desktop/h1", "styleType": "TEXT", "value": {
                "fontName": {"family": "Pretendard", "style": "ExtraBold"}, "fontSize": 40,
                "lineHeight": {"unit": "PIXELS", "value": 48}, "letterSpacing": {"unit": "PERCENT", "value": 0},
                "textCase": "ORIGINAL", "textDecoration": "NONE",
                "boundVariables": {"fontSize": {"type": "VARIABLE_ALIAS", "id": "var:size"}}
            }},
            {"id": "s:3", "name": "3/label", "styleType": "TEXT", "value": {
                "fontName": {"family": "Noto Sans KR", "style": "Medium"}, "fontSize": 13,
                "lineHeight": {"unit": "PERCENT", "value": 150}, "letterSpacing": {"unit": "PERCENT", "value": -4},
                "textCase": "ORIGINAL", "textDecoration": "NONE", "boundVariables": {}
            }},
            {"id": "e:0", "name": "card", "styleType": "EFFECT", "value": [
                {"type": "INNER_SHADOW", "visible": true, "offset": {"x": 1, "y": 1}, "radius": 2, "spread": 0, "color": {"r": 1, "g": 1, "b": 1, "a": 1}},
                {"type": "DROP_SHADOW", "visible": false, "offset": {"x": 0, "y": 9}, "radius": 9, "spread": 0, "color": {"r": 0, "g": 0, "b": 0, "a": 1}},
                {"type": "LAYER_BLUR", "visible": true, "radius": 4}
            ]},
            {"id": "e:4", "name": "desktop/card", "styleType": "EFFECT", "value": [
                {"type": "DROP_SHADOW", "visible": true, "offset": {"x": 0, "y": 8}, "radius": 24, "spread": -4, "color": {"r": 0, "g": 0, "b": 0, "a": 0.15}}
            ]},
            {"id": "e:blur", "name": "glow", "styleType": "EFFECT", "value": [
                {"type": "LAYER_BLUR", "visible": true, "radius": 4}
            ]}
        ],
        "usedRemoteVariables": [],
        "localComplete": true,
        "usedRemoteComplete": true
    }))
    .expect("variable snapshot");

    let output = generate_devup_json(&variables, ThemeScope::File).expect("devup theme");
    let json: serde_json::Value = serde_json::from_str(&output.json).expect("json");
    assert_eq!(
        json["theme"]["typography"],
        json!({
            "h1": [
                {"fontFamily": "Pretendard", "fontStyle": "italic", "fontWeight": 700, "fontSize": "24px",
                 "textDecoration": "underline", "textTransform": "uppercase", "lineHeight": "normal", "letterSpacing": "0.5px"},
                null,
                null,
                null,
                // the token is named as every `$token` here is, by the
                // variable's last path segment
                {"fontFamily": "Pretendard", "fontWeight": 800, "fontSize": "$sizeLg", "lineHeight": "48px", "letterSpacing": "0em"}
            ],
            // one slot only is the value alone, wherever the slot was
            "label": {"fontFamily": "Noto Sans KR", "fontWeight": 500, "fontSize": "13px", "lineHeight": 1.5, "letterSpacing": "-0.04em"}
        })
    );
    // A blur is not a shadow; an effect style with only blurs is no entry.
    assert_eq!(
        json["theme"]["shadow"],
        json!({"default": {"card": ["inset 1px 1px 2px 0 #ffffff", null, null, null, "0 8px 24px -4px #00000026"]}})
    );
}

#[test]
fn reports_alias_cycles_instead_of_inventing_values() {
    let variables: VariableSnapshot = serde_json::from_value(json!({
        "collections": [{"id": "c", "name": "C", "defaultModeId": "m", "modes": [{"modeId": "m", "name": "Default"}]}],
        "variables": [
            {"id": "a", "name": "A", "resolvedType": "COLOR", "variableCollectionId": "c", "valuesByMode": {"m": {"type": "VARIABLE_ALIAS", "id": "b"}}},
            {"id": "b", "name": "B", "resolvedType": "COLOR", "variableCollectionId": "c", "valuesByMode": {"m": {"type": "VARIABLE_ALIAS", "id": "a"}}}
        ],
        "styles": [], "usedRemoteVariables": [], "localComplete": true, "usedRemoteComplete": true
    }))
    .unwrap();

    let output = generate_devup_json(&variables, ThemeScope::File).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .any(|item| item.code == "DEVUP_THEME_ALIAS_CYCLE")
    );
    assert!(!output.json.contains("\"a\""));
    assert!(!output.json.contains("\"b\""));
}

#[test]
fn resolves_token_conflicts_deterministically_across_input_orders() {
    let first = generate_devup_json(&conflicting_variables(false), ThemeScope::File).unwrap();
    let reversed = generate_devup_json(&conflicting_variables(true), ThemeScope::File).unwrap();

    assert_eq!(first.json, reversed.json);
    assert!(first.json.contains("\"primary\": \"#ff0000\""));
    assert_eq!(first.conflicts, reversed.conflicts);
    assert_eq!(first.conflicts.len(), 1);
    assert_eq!(first.conflicts[0].token, "primary");
    assert_eq!(first.conflicts[0].mode, "default");
    assert_eq!(first.conflicts[0].winner_variable_id, "var:a");
    assert_eq!(
        first.conflicts[0]
            .candidates
            .iter()
            .map(|candidate| candidate.variable_id.as_str())
            .collect::<Vec<_>>(),
        vec!["var:a", "var:z"]
    );
    assert_eq!(
        first.conflicts[0].candidates[0].collection_name,
        "Foundation"
    );
    assert_eq!(first.conflicts[0].candidates[0].raw_name, "Color/Primary");
    assert_eq!(first.conflicts[0].candidates[0].mode_id, "mode:default");
    assert_eq!(
        first.conflicts[0].candidates[0].value_hash,
        "db5d24235bf240470a890fbfe94cc1cc2a17489e1aa65c9ede395d8be4d0f71a"
    );
    assert_eq!(
        first.conflicts[0].candidates[1].value_hash,
        "904aff52bad2d6d9406b096ad03367bbe25737d47d36eb06f74411bd126f72b9"
    );
    assert!(
        first
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "DEVUP_THEME_TOKEN_CONFLICT")
    );
}

fn conflicting_variables(reversed: bool) -> VariableSnapshot {
    let mut variables = vec![
        json!({
            "id": "var:z",
            "name": "Legacy/Primary",
            "resolvedType": "COLOR",
            "variableCollectionId": "collection:1",
            "codeSyntax": {},
            "valuesByMode": {"mode:default": {"r": 0, "g": 0, "b": 1, "a": 1}}
        }),
        json!({
            "id": "var:a",
            "name": "Color/Primary",
            "resolvedType": "COLOR",
            "variableCollectionId": "collection:1",
            "codeSyntax": {},
            "valuesByMode": {"mode:default": {"r": 1, "g": 0, "b": 0, "a": 1}}
        }),
    ];
    if reversed {
        variables.reverse();
    }
    serde_json::from_value(json!({
        "collections": [{
            "id": "collection:1",
            "name": "Foundation",
            "defaultModeId": "mode:default",
            "modes": [{"modeId": "mode:default", "name": "Default"}]
        }],
        "variables": variables,
        "styles": [],
        "usedRemoteVariables": [],
        "localComplete": true,
        "usedRemoteComplete": true
    }))
    .unwrap()
}

#[test]
fn node_scope_keeps_used_alias_dependencies_and_excludes_unused_resources() {
    let snapshot: VariableSnapshot = serde_json::from_value(json!({
        "collections": [{
            "id": "collection:1", "name": "Foundation", "defaultModeId": "mode:default",
            "modes": [{"modeId": "mode:default", "name": "Default"}]
        }],
        "variables": [
            {
                "id": "var:primary", "name": "Color/Primary", "resolvedType": "COLOR",
                "variableCollectionId": "collection:1",
                "valuesByMode": {"mode:default": {"r": 1, "g": 0, "b": 0, "a": 1}}
            },
            {
                "id": "var:accent", "name": "Color/Accent", "resolvedType": "COLOR",
                "variableCollectionId": "collection:1",
                "valuesByMode": {"mode:default": {"type": "VARIABLE_ALIAS", "id": "var:primary"}}
            },
            {
                "id": "var:unused", "name": "Color/Unused", "resolvedType": "COLOR",
                "variableCollectionId": "collection:1",
                "valuesByMode": {"mode:default": {"r": 0, "g": 0, "b": 1, "a": 1}}
            }
        ],
        "styles": [
            {"id": "style:used", "name": "Body/Used", "styleType": "TEXT", "value": {"fontSize": "16px"}},
            {"id": "style:unused", "name": "Body/Unused", "styleType": "TEXT", "value": {"fontSize": "12px"}}
        ],
        "usedRemoteVariables": [],
        "usedVariableIds": ["var:accent"],
        "usedStyleIds": ["style:used"],
        "localComplete": true,
        "usedRemoteComplete": true
    }))
    .unwrap();

    let node = generate_devup_json(&snapshot, ThemeScope::Node).unwrap();
    let file = generate_devup_json(&snapshot, ThemeScope::File).unwrap();

    assert!(node.json.contains("\"accent\": \"#ff0000\""));
    assert!(node.json.contains("\"primary\": \"#ff0000\""));
    assert!(node.json.contains("\"bodyUsed\""));
    assert!(!node.json.contains("\"unused\""));
    assert!(!node.json.contains("\"bodyUnused\""));
    assert_eq!(node.completeness, Completeness::UsedTokens);
    assert!(file.json.contains("\"unused\": \"#0000ff\""));
    assert!(file.json.contains("\"bodyUnused\""));
    assert_eq!(file.completeness, Completeness::FullLocalPlusUsedRemote);
}

#[test]
fn unresolved_variables_do_not_remove_independent_valid_tokens() {
    let snapshot: VariableSnapshot = serde_json::from_value(json!({
        "collections": [{
            "id": "c", "name": "C", "defaultModeId": "m",
            "modes": [{"modeId": "m", "name": "Default"}]
        }],
        "variables": [
            {"id": "a", "name": "A", "resolvedType": "COLOR", "variableCollectionId": "c", "valuesByMode": {"m": {"type": "VARIABLE_ALIAS", "id": "b"}}},
            {"id": "b", "name": "B", "resolvedType": "COLOR", "variableCollectionId": "c", "valuesByMode": {"m": {"type": "VARIABLE_ALIAS", "id": "a"}}},
            {"id": "valid", "name": "Color/Valid", "resolvedType": "COLOR", "variableCollectionId": "c", "valuesByMode": {"m": {"r": 0, "g": 1, "b": 0, "a": 1}}},
            {"id": "missing", "name": "Color/Missing", "resolvedType": "COLOR", "variableCollectionId": "missing-c", "valuesByMode": {"m": {"r": 1, "g": 0, "b": 0, "a": 1}}}
        ],
        "styles": [], "usedRemoteVariables": [],
        "localComplete": true, "usedRemoteComplete": true
    }))
    .unwrap();

    let output = generate_devup_json(&snapshot, ThemeScope::File).unwrap();

    assert!(output.json.contains("\"valid\": \"#00ff00\""));
    assert_eq!(
        output
            .unresolved_variables
            .iter()
            .map(|unresolved| (
                unresolved.variable_id.as_str(),
                unresolved.mode_id.as_deref(),
                unresolved.reason.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("a", Some("m"), "alias-cycle-or-missing-target"),
            ("b", Some("m"), "alias-cycle-or-missing-target"),
            ("missing", None, "collection-missing")
        ]
    );
}
