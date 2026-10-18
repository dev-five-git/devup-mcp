

/// The module is what `devup_figma_export` hands back as `responsiveTsx`, and
/// it is assembled from the merge rather than emitted by it — imports on one
/// side, the tree on the other. Nothing here checked that the two agree on a
/// real screen: the merge's own tests stop at `tsx`, and the export test that
/// covers this wiring runs against a stub. An element drawn but not imported
/// is a file that does not compile, and it would only be discovered after a
/// capture had been paid for.
#[test]
fn every_element_the_module_draws_is_one_it_imports() {
    let Some(merged) = merged() else {
        eprintln!("no capture; skipping");
        return;
    };

    let primitives = merged.primitives();
    let components = merged.referenced_components();

    // The two lists are the one set of components, split and not overlapping.
    let declared = primitives
        .iter()
        .chain(components.iter())
        .map(|name| (*name).to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        declared.len(),
        primitives.len() + components.len(),
        "a name must be a primitive or a component of this design, not both"
    );

    let drawn = rendered_tags(&merged.tsx);
    assert!(!drawn.is_empty(), "the screen draws something");
    let unimported = drawn.difference(&declared).collect::<Vec<_>>();
    assert!(
        unimported.is_empty(),
        "drawn but not imported: {unimported:?}"
    );

    let module = merged.module("AboutPage");

    // Imports first, then one default export holding the tree.
    let export = module
        .find("export default function AboutPage()")
        .expect("a default export named after the screen");
    for line in module[..export]
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        assert!(
            line.starts_with("import "),
            "only imports may precede the export: {line}"
        );
    }
    assert_eq!(
        module.matches("export default function").count(),
        1,
        "one screen, one default export"
    );
    assert!(
        module.contains(&merged.tsx),
        "the module must carry the merged tree unchanged"
    );

    // Each component is imported once, from its own file.
    let imports = module[..export]
        .lines()
        .filter(|line| line.starts_with("import "))
        .collect::<Vec<_>>();
    let unique = imports.iter().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), imports.len(), "no import written twice");
    for name in &components {
        assert!(
            module.contains(&format!("import {{ {name} }} from '@/components/{name}'")),
            "{name} must be imported from its own file"
        );
    }
    if !primitives.is_empty() {
        assert!(
            module.contains("from '@devup-ui/react'"),
            "primitives come from devup-ui"
        );
    }
}
