use devup_mcp::theme::{Completeness, ThemeScope, VariableSnapshot, generate_devup_json};
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
        "styles": [
            {"id": "style:text", "name": "Heading/H1", "styleType": "TEXT", "value": {
                "fontFamily": "Pretendard", "fontSize": "32px", "fontWeight": 700, "lineHeight": 1.3
            }},
            {"id": "style:effect", "name": "Elevation/Card", "styleType": "EFFECT", "value": "0 4px 12px #0000001a"}
        ],
        "usedRemoteVariables": [],
        "localComplete": true
    }))
    .expect("variable snapshot");

    let output = generate_devup_json(&variables, ThemeScope::File).expect("devup theme");
    assert_eq!(output.completeness, Completeness::FullLocalPlusUsedRemote);
    assert_eq!(
        output.json,
        concat!(
            "{\n",
            "  \"theme\": {\n",
            "    \"colors\": {\n",
            "      \"darkMode\": {\n",
            "        \"accent\": \"#3291ff\",\n",
            "        \"primary\": \"#3291ff\"\n",
            "      },\n",
            "      \"light\": {\n",
            "        \"accent\": \"#0070f3\",\n",
            "        \"primary\": \"#0070f3\"\n",
            "      }\n",
            "    },\n",
            "    \"typography\": {\n",
            "      \"headingH1\": {\n",
            "        \"fontFamily\": \"Pretendard\",\n",
            "        \"fontSize\": \"32px\",\n",
            "        \"fontWeight\": 700,\n",
            "        \"lineHeight\": 1.3\n",
            "      }\n",
            "    },\n",
            "    \"length\": {\n",
            "      \"darkMode\": {\n",
            "        \"gutter\": \"20px\"\n",
            "      },\n",
            "      \"light\": {\n",
            "        \"gutter\": \"16px\"\n",
            "      }\n",
            "    },\n",
            "    \"shadow\": {\n",
            "      \"default\": {\n",
            "        \"elevationCard\": \"0 4px 12px #0000001a\"\n",
            "      }\n",
            "    }\n",
            "  }\n",
            "}\n"
        )
    );
    assert_eq!(output.counts.variables, 3);
    assert_eq!(output.counts.styles, 2);
}

#[test]
fn reports_alias_cycles_instead_of_inventing_values() {
    let variables: VariableSnapshot = serde_json::from_value(json!({
        "collections": [{"id": "c", "name": "C", "defaultModeId": "m", "modes": [{"modeId": "m", "name": "Default"}]}],
        "variables": [
            {"id": "a", "name": "A", "resolvedType": "COLOR", "variableCollectionId": "c", "valuesByMode": {"m": {"type": "VARIABLE_ALIAS", "id": "b"}}},
            {"id": "b", "name": "B", "resolvedType": "COLOR", "variableCollectionId": "c", "valuesByMode": {"m": {"type": "VARIABLE_ALIAS", "id": "a"}}}
        ],
        "styles": [], "usedRemoteVariables": [], "localComplete": true
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
