#[tokio::test]
async fn effective_props_wire_native_handlers_and_navigation() -> anyhow::Result<()> {
    let output = validate(json!({"tsx": r#"import RouteLink from 'next/link'; const Header = () => <nav><Text as={RouteLink} props={{href:'/x', ['aria-current']: active ? 'page' : undefined}} _hover={{color:'red'}} transition="color .2s">Company</Text><Box as="button" props={{onClick: () => open()}}>Open</Box><Text as="a" {...{href:'/x'}} _hover={{transition:'color .2s'}}>Other</Text></nav>"#})).await?;
    assert!(!has_rule(&output, "interaction-unmapped"), "{output}");
    assert!(!has_rule(&output, "hover-transition"), "{output}");
    assert_eq!(output["navigation"]["sources"][0]["report"]["coverage"]["wiredCount"], 3);
    Ok(())
}

#[tokio::test]
async fn sort_findings_produce_one_action_across_bundle_files() -> anyhow::Result<()> {
    let output = validate(json!({"files": [
        {"path": "one.tsx", "content": "<Box p={0} boxSize=\"8px\" />"},
        {"path": "two.tsx", "content": "<Box props={{}} pr=\"16px\" />"}
    ]})).await?;
    assert_eq!(output["nextAction"]["fixable"], 2);
    assert_eq!(output["nextAction"]["command"], "bun run lint:fix");
    Ok(())
}

#[tokio::test]
async fn chondrogen_header_props_preserve_navigation_and_hover_states() -> anyhow::Result<()> {
    let source = r#"
        import Link from 'next/link';
        export function Header() {
            return <nav>
                <Text as={Link} props={{ href: '/x', 'aria-current': isActive('/x') ? 'page' : undefined }}
                    _hover={{ color: '$primary' }} transition="color 0.2s ease">Company</Text>
            </nav>;
        }
    "#;
    let output = structural(source, "src/components/layout/Header.tsx").await?;
    assert!(!has_rule(&output, "nav-current"), "{output}");
    assert!(!has_rule(&output, "hover-transition"), "{output}");
    let missing_states = source
        .replace("'aria-current': isActive('/x') ? 'page' : undefined", "")
        .replace("transition=\"color 0.2s ease\"", "");
    let output = structural(&missing_states, "src/components/layout/Header.tsx").await?;
    assert!(has_rule(&output, "nav-current"), "{output}");
    assert!(has_rule(&output, "hover-transition"), "{output}");
    Ok(())
}

#[tokio::test]
async fn chondrogen_header_desktop_nav_has_no_narrow_fixed_columns() -> anyhow::Result<()> {
    for display in ["['none', null, null, null, 'flex']", "'flex'"] {
        let output = validate(json!({"tsx": format!(r#"
            <Flex as="nav" display={{{display}}}>
                <Box w="240px">Company</Box><Box w="240px">Research</Box>
            </Flex>
        "#)})).await?;
        let risks = output["layoutRisks"].as_array().unwrap();
        assert_eq!(
            risks.iter().any(|risk| risk["rule"] == "narrow-fixed-columns"),
            display == "'flex'",
            "{output}"
        );
    }
    Ok(())
}
