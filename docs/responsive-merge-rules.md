# Breakpoint merging, as the plugin does it

The rules below are read off the plugin's source at the commit this repo's
corpus pins, `243db65` of `dev-five-git/devup-figma-plugin` — chiefly
`src/codegen/responsive/index.ts` (`getBreakpointByWidth`,
`mergePropsToResponsive`, `optimizeResponsiveValue`) and
`ResponsiveCodegen.ts`. Where a claim came from reading output rather than
source it says so.

They are checked against two screens in `devup-Test`, chosen because they take
opposite branches of the merge.

- **`notice`** (node `422:6865`, widths 360 / 992 / 1920) — the widths disagree
  in shape, so almost nothing merges. Its four arrays are all `display`, and
  there is not one merged value in the file.
- **`popup`** (node `422:5758`) — the widths agree in shape everywhere, so this
  is the screen that shows value merging: 15 arrays, no `display` at all.

A screen tends to take one branch wholesale rather than a mixture, which is why
one screen alone was a misleading sample.

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

Five slots, `[mobile, mid, tablet, mid, PC]`. A slot is written only when it
changes what is in effect, because `null` is not "no value" but "whatever the
slot before it said". So the array records *transitions*, not widths, and
trailing nulls are dropped:

```tsx
display={["none", null, "flex"]}   // absent on mobile, present from tablet up
display={[null,  null, "none"]}    // present on mobile, absent from tablet up
```

**Which slot a width occupies is decided by how wide its frame is, not by what
it is called.** The plugin's `getBreakpointByWidth` places a frame in the first
band its width fits:

| Slot | 0 | 1 | 2 | 3 | 4 |
|---|---|---|---|---|---|
| name | mobile | sm | tablet | lg | pc |
| width | ≤ 480 | ≤ 768 | ≤ 992 | ≤ 1280 | rest |

`notice` is drawn at 360 / 992 / 1920, so its three frames land on 0 / 2 / 4.
`popup`'s frame named `tablet` is under 768 and lands on slot **1**. Reading
the frame's name and assuming slot 2 puts every value of that screen a band too
wide.

**A prop that stops has to say so.** Because `null` inherits, a prop cannot
simply be dropped at a wider width — the narrower value would carry. The
reference writes `"initial"`, and spends one only where something would
otherwise be inherited:

```tsx
pl={["36.5px", "initial"]}                    // mobile sets it, nothing else does
px={[null, "184px", null, null, "initial"]}   // slot 0 is null: nothing to clear yet
```

Two limits of that device are worth knowing, because they are the reference's
and this repo follows them rather than quietly improving on them.

*Only layout props are cleared.* `SPECIAL_PROPS_WITH_INITIAL` lists display,
position, transform, `w`/`h`, `textAlign`, the flex and grid props, the offsets,
overflow, and every padding and margin. A `bg` that stops being set is left to
inherit.

*Exactly one `"initial"` is placed*, at the first drawn width after the last
value, and the array is cut there. So a prop that is set, dropped, then set
again at a wider width cannot be expressed — the drop is lost. Neither screen
here does that, but a design could.

**A first value that is already the default is not written.** `flexDir="row"`,
`alignItems`/`justifyContent="flex-start"` and `gap="0"` are dropped from slot
0, so `["row", null, "column"]` is written `[null, null, "column"]`. The
padding and margin entries of that table are commented out upstream.

This is the whole reason the two branches below are not interchangeable. Values
that move can become arrays; a subtree whose *shape* moves cannot, because
props appear and disappear with the nodes that carried them.

## The element can merge too

Where the widths draw different elements, the merge does not keep both. The
`popup` root is a `VStack` at two widths and a `Flex` at the third, and it comes
out as one `VStack` with the difference demoted to a prop:

```tsx
<VStack flexDir={["column", null, null, null, "row"]} ... >
```

Props are unioned rather than reconciled. Mobile sets `pl` and `pr`, tablet sets
`px`, desktop sets neither; all three survive as three arrays, and nothing tries
to rewrite `pl`/`pr` into `px`.

## Two ways a subtree can differ

These are not two equal paths. A screen drawn at three widths is meant to be
the same tree three times, and merging into arrays is what should happen; the
other branch is what saves an export when the design drifted. Of this screen's
four children, three merge and one does not:

```
[0] main banner   mobile 3 children / tablet 2 / desktop 2   ← the odd one
[1] Header        1 / 1 / 1
[2] section       1 / 1 / 1
[3] Footer        1 / 1 / 1
```

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

The two logos are wrapped in a frame on desktop and left loose on mobile — the
same intent grouped two ways, which is a drift in the file rather than a
difference the screen means to express. It shows in the output: the desktop
wrapper folds into `maskImage="url('/icons/Frame 1000014364.svg')"` while
mobile emits two separately placed logos. Read this branch as the cost of that
drift, not as the feature. A design whose widths agree in shape never reaches
it.

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
`merged_props` folds differing values into an expression, and `unrepresented`
collects what could not be represented.

`responsive.rs` holds the breakpoint side. `breakpoints` finds the widths a
snapshot carries and `divergences` names every place their shapes part company
— the input to the toggle branch. `slot_of_width` is `getBreakpointByWidth`,
and `merge_slots` is `mergePropsToResponsive` folded together with
`optimizeResponsiveValue`, carrying both of its limits deliberately.
`tests/responsive_merge.rs` checks it against all 15 arrays of `popup`, all 4
of `notice`, and the five widths the plugin's own unit test pins.

`Expression::Responsive` now carries its slots as a vector rather than a fixed
pair, so the two callers can write different shapes: the viewport path keeps
writing `[mobile, null, null, null, PC]`, which the pinned corpus requires,
while the breakpoint path writes whatever `merge_slots` returns.

The join is `merge_breakpoints`, ported from
`ResponsiveCodegen.generateMergedCode`. Its shape is worth stating because it is
not what reading the output suggests:

- **Children are paired by name, not by index.** Each width's children are
  bucketed by name, and for a name held by several children they are paired by
  position *within that name*. So a node inserted at one width shifts nothing.
- **The toggle is not a separate branch.** A child missing at some width is not
  left out; the width is given a *copy of a present sibling's tree* with
  `display: 'none'` added. The copies then go through the ordinary merge, and
  the `display` array falls out of `merge_slots` like any other prop. That is
  why `notice`'s toggles need no special case here — they are what this
  function already does.
- Which also explains `["flex", null, "none"]` against `[null, null, "none"]`:
  the difference is only whether the shown copy's own props carried a
  `display`, not a decision about how to hide it. The plugin keeps `display` in
  a node's props and picks the element from it; this repo picks the element
  first, so `restore_implied_display` puts the value back before merging. Not
  doing so is a real defect rather than a formatting difference — the hidden
  width would clear the others to `"initial"`, and `display: initial` is
  `inline`, not `flex`.

- **An instance whose component is only a shape is spelled out, not
  referenced.** `isAssetLeafTree` — a childless `Image` with a `src` or `Box`
  with a `maskImage`. That is why the plugin writes `{/* <Logo /> */}` and then
  a `Box`: it recognised the component and declined to reference it. Without
  this the output carries `<Logo />` and `<Icons />` and asks for two component
  files whose whole body is one masked box.

`tests/responsive_screen.rs` runs the join on the `notice` capture and checks it
against the plugin's answer: the four `display` arrays, the slot placement, what
is referenced against what is inlined, and every element at its nesting.

## Reaching it

`responsiveTsx` is an export output beside `tsx` and `componentTsx`. It carries
the whole module — the `@devup-ui/react` import, one `@/components/…` import per
component the screen references, and the screen as a default export — because a
fragment would leave the caller to work out which of the two import lines each
element belongs on. It is also produced whenever `tsx` is asked for and the
capture holds more than one width, since a caller cannot tell from a node id
whether the responsive form exists.

Alongside it the export names `responsiveSlots` (which slot each width took),
`responsiveImports` and `responsiveComponents` (the two import lines, already
separated), and `responsiveUnrepresented` when the widths asked for something a
single tree cannot say. `crates/devup-mcp/tests/responsive_export.rs` drives the
real tool over a stubbed upstream and checks that path end to end.

## Where this repo differs, and why

Two differences from the reference are deliberate. Both are recorded because a
difference is a question until someone says how it was settled.

**The first width is the narrowest, not the first in the file.**
`categorizeChildren` walks the Section's children in their canvas order, so the
plugin's "first" width is wherever the designer happened to put a frame —
`notice` is authored desktop-first and `popup` mobile-first, and that alone
decides which width lends the merged node its element name. This repo orders by
width. The visible effect is that a pair of regions shown at opposite widths
comes out in the other order; neither is ever visible at the same time, so it
is order between two things that never meet.

**A component prop that differs by width is reported.** `<Footer />` wants
`mobile`, `tablet` and `desktop`, and devup-ui reads an array only where it
applies CSS, so no value is right. The reference silently keeps whichever width
came first, which the design owner reads as an omission rather than a decision.
The widest is kept here too, so the output matches, but the loss is named in
`MergedScreen::unrepresented` instead of passing quietly.

**A positioned shape is one element, not two.** The reference wraps the inlined
shape in a `Box` that carries only its placement. Merging the placement into
the shape's own `Box` draws the same thing with two elements fewer, which is
the only place the element counts differ.

**A selector block says only what the state changes.** The reference writes
`_hover={{ "gap": size === 'md' && "10px" }}` where this writes
`{ md: varient === 'ghost' && "10px" }[size]`, and the second is the narrower
claim: at `md` the base gap is `8px` for `ghost` and `10px` for everything else,
so hover changes it for `ghost` alone.

The difference comes from one line of `createNestedVariantProp`. It computes
each state's delta per combination and then folds the combinations together,
and when a branch has only one combination *carrying a value* it treats them as
agreeing and collapses to that value — filling in the combinations that had
none. Here that is harmless because the value it fills in is the value those
combinations already had. It is harmless by coincidence: were the base `8px`
elsewhere at `md`, hover would be given a change the design never asked for.

The same collapse was written here first and the pinned corpus rejected it —
`border={variant === 'white' && …}` became `border="solid 1px …"`, handing a
border to the four variants that refuse one. Presence is asked of the record
rather than of the value for that reason, and the rule is not suspended for
selector blocks.

**`display` is never cleared to `"initial"`.** `initial` is the value the CSS
specification gives a property, not the value the element has. For the other
forty props in the set those coincide — `w` is `auto`, `p` is `0`, `pos` is
`static`, `overflow` is `visible` — and clearing to `initial` is exactly right.
`display` is the one where they part: an element's display comes from the
user-agent stylesheet, so `div` is `block` and `img` is `inline`, while the
property's initial value is `inline` for all of them. A node hidden at a narrow
width and shown at a wider one would come back inline: a `Box` would stop being
a block, and a `Text` — which devup-ui renders as a `p` — with it.

`revert` is the keyword that means what is wanted here, and it cannot be used
either. It rolls the cascade back past the author origin, and that is where
devup-ui puts `VStack`'s own `display: flex`, so reverting would take that with
it. The value is written out instead:

| element | renders as | written |
|---|---|---|
| `Flex` `VStack` `Center` | `div` + class | `flex` |
| `Grid` | `div` + class | `grid` |
| `Box` | `div` | `block` |
| `Text` | `p` | `block` |
| `Image` | `img` | `inline` |

Neither screen kept here reaches the case, so this is a correction made from
reading the rule rather than from a disagreement with an answer. The design
owner confirmed the `initial` in the reference was a mistake rather than
intent.
