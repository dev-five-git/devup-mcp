# Prop Ordering (Feedback 45)

The published eslint-plugin-devup 2.1.0 oxlint config uses
perfectionist/sort-jsx-props with case-sensitive alphabetical ascending ordering,
reserved key/ref first, then unknown props. Generated TSX is not consistently
sorted: render stages append layout, paint, provenance and interaction props.

A global source rewrite is not cheap or safe: spreads and duplicate attributes
are evaluation/override boundaries, and project configs can override the rule.
Generation is left unchanged; no snapshots need refreshing. The validator counts
adjacent inversions within identifier-only runs separated by spreads/namespaced
attributes, and offers one lint:fix action. Counts describe detected inversions,
not every diagnostic oxlint would emit. It does not claim to validate prettier,
import order or custom perfectionist configuration. The project fixer remains
authoritative for all formatting and sorting.

Export exposes this action before its optional project-theme gate and keeps an
existing tool nextAction (for example a job continuation) intact. Formatting has
its own single nextAction in that case. Validation aggregates across bundle
files, without individual prop-reordering instructions.
