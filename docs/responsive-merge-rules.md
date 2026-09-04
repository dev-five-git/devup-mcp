# Breakpoint merging, as the plugin does it

Measured against `devup-Test` node `422:6865` (`desktop`), whose parent is the
`notice` Section holding `desktop` / `tablet` / `mobile`. The plugin was asked
for that one frame and answered with four outputs; what follows is what they
establish. Every claim here is read off those four, not inferred.

## The four outputs

| Output | What it is |
|---|---|
| Pure Code | the selected frame, primitives only, every instance expanded |
| desktop | the same frame with instances left as `<Header />`, `<Tab />` |
| desktop - Components | the definitions of those components, with their prop types |
| notice - Responsive | all three widths merged, components kept |

Only the fourth carries `display` arrays. The first three describe one width.

## Definitions cannot be derived

The difference between Pure Code and the component-applied output gives a
component's *body*, so it looked as though definitions did not need their own
output. They do:

```tsx
export interface HeaderProps {
    property1: 'scroll' | 'transparent' | 'mobileTranspa' | 'mobileScroll'
}
```

That union comes from the component set's variants. Three of those four
variants appear nowhere in a screen that uses `property1="transparent"`, so no
amount of diffing recovers them. The same holds for `FooterProps`, and for
`Icons`, whose union names fifty-odd glyphs whose call site mentions one.

A definition also carries what a call site cannot: `_hover` / `_active` /
`_selected` blocks, and per-variant prop maps written as
`bg={{ scroll: "$headerBg", mobileScroll: "$headerBg" }[property1]}`.

## The array

Five slots, `[mobile, null, tablet, null, PC]`. With two widths it is
`[mobile, null, null, null, PC]` — already how `Expression::Responsive`
renders. What appears in the reference is three slots, because this design's
tablet and desktop agree on every value that differs from mobile, so slot 2
covers tablet upward and slots 3 and 4 are dropped rather than written null.

```tsx
display={["none", null, "flex"]}   // absent on mobile, present from tablet up
display={[null,  null, "none"]}    // present on mobile, absent from tablet up
```

## Two ways a subtree can differ

**Structure matches → merge, and let differing values become arrays.** The
`Header` instance is identical across all three widths, so it appears once and
is not toggled at all:

```tsx
<Box left="0px" pos="absolute" top="0px" w="100%">
    <Header property1="transparent" />
</Box>
```

**Structure differs → keep both, toggle with `display`.** The banner is not one
node with responsive values; it is two nodes, each shown at its own widths. The
capture says why — the same-named frame is shaped differently:

```
mobile  'main banner' kids=3   [Frame…289, Logo, Logo]
desktop 'main banner' kids=2   [Frame…289, Frame…364]
```

The mobile banner also holds two absolutely-placed logos the desktop one does
not, and its text sits in a `pos="absolute"` stack rather than a centred
column. There is no alignment to merge, so both survive.

The same split appears again in the content section: desktop puts the tabs
beside the search box in a `Flex`, mobile stacks them in a `VStack` with the
search box first, and both are emitted with opposite `display` arrays.

## What the reference does not do

Component props are not responsive. `<Footer property1="desktop" />` stays
`desktop` at every width even though `FooterProps` admits `'mobile'` and
`'tablet'` and the definition lays out all three. Passing an array there does
nothing, and the design owner reads this as the plugin's omission rather than
intended behaviour — worth knowing before treating it as ground truth.

## Where this lands in the code

`variant.rs` already merges trees for viewport *variants* of a component set:
`same_rendered_structure` decides whether two trees are the same shape,
`merged_props` folds differing values into an expression, `unrepresented`
collects what could not be represented, and `Expression::Responsive` renders
the five-slot array. What is missing is the other entry: the same machinery
driven by sibling frames in a Section rather than by variants of a set, and a
third slot in the array.
