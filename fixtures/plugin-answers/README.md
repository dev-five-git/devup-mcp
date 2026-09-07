# What the plugin answered

Not "the correct output". These are what `devup-figma-plugin` returned when
asked for real screens in `devup-Test`, and they are kept because they are the
only account of how it behaves on one, not because they are known to be right.

The distinction matters. The plugin is the reference this repo aims to match or
beat, and its author does not vouch for every line here. Treat a difference as a
question, the same way a difference against the pinned corpus is a question, and
say which way it was settled.

## The screens

`notice/` and `popup/` are the two kept in full, chosen to sit at opposite ends
of the merge so that between them they exercise both of its branches. `button/`
is not a screen at all but a component set, kept for what it says about props a
call site may not pass. `about/` holds one output rather than a set, and its
section says how it got here, which is not the way the others did.

### `notice/` — where the widths part company

Node `422:6865`, the `desktop` width. Four outputs:

| File | The plugin's tab | What it shows |
|---|---|---|
| `pure.tsx` | Pure Code | the frame with every instance expanded to primitives |
| `with-components.tsx` | desktop | the same frame with instances left as `<Header />` |
| `components.tsx` | desktop - Components | the definitions of those components |
| `responsive.tsx` | notice - Responsive | all three widths merged |

Its widths are 360 / 992 / 1920. They disagree in *shape*, so almost nothing
merges: the four arrays in `responsive.tsx` are all `display`, and there is not
one merged value in the file.

### `popup/` — where they agree and only the numbers move

Node `422:5758`. Its widths agree in shape everywhere, so this is the screen
that exercises value merging — 15 arrays, no `display` at all. It has no
components, so its Pure Code and `desktop` tabs are identical and only one is
kept.

| File | The plugin's tab |
|---|---|
| `pure-mobile.tsx` | Pure Code at the `mobile` width |
| `pure-tablet.tsx` | Pure Code at the `tablet` width |
| `pure-desktop.tsx` | Pure Code at the `desktop` width |
| `responsive.tsx` | popup - Responsive |

Together the two screens say that a merge has two branches and that a screen
usually takes one of them wholesale, not a mixture.

### `about` — the widths that go back, kept as tests and not as a file

Node `422:3798`, and `responsive.tsx` is its `about - Responsive` output.

**It arrived differently from the others, and that is worth knowing.** The three
above came from the plugin to a file. This one came through a chat window, and
for a while it was deliberately not kept: a reference is only useful as a judge
if it is exact, and a 1,400-line file copied by hand is a judge that cannot be
trusted, which is worse than none.

What changed is not the route but the checking. Six of its arrays had already
been read and locked as tests — `responsive_merge.rs::the_about_screen_needs_every_slot`
— before the file existed, so they could be used to check the file rather than
the other way round. All six are present in it, along with its imports, both
component references, the literal `display="none"`, and a single default export;
its indentation steps by four, as `notice`'s does, which is what the comparison
reads nesting from. A paste that had been truncated or mangled would have failed
one of those. This one did not.

So it is kept, and `responsive_screen.rs::the_about_screen_matches_the_answer_when_both_are_present`
decides the comparison rather than a person reading two files side by side. It
still waits on the capture at `fixtures/local-screens/about-family.json` and
skips until that arrives. Do not reformat this file: the nesting is the
comparison.

`about - Components` is still worth adding if it is to hand. A definitions
output cannot be recovered from the others, and the union behind
`<Header status="landing" />` exists nowhere else.

It earns an entry because it is the first answer whose widths *return*. `notice`
only ever toggles one way and `popup` only ever moves a number forward, so
between them every array stops by slot 2. `about` has both a region shown at
tablet and hidden again at desktop, and values that come back at desktop to what
mobile said:

```tsx
display={["none", null, "flex", null, "none"]}   // tablet only
w={["770px", null, "778px", null, "770px"]}      // tablet is the odd one
textAlign={[null, null, "right", null, "initial"]}
```

The closing slot in each is not redundant with the opening one. Leaving it off
would inherit the tablet value, so these are the arrays that need all five
slots, and they are the reason the merge cannot stop at the last *changed* slot.
The `textAlign` row also confirms that the cleared set reaches beyond spacing
and layout — this repo already had it there, and the answer agrees.

Two more things it settles:

- **A node hidden at every width is emitted, not dropped.** The output opens
  with `<Box display="none" … />`, a literal rather than an array, which is
  three widths all reporting `display: 'none'` and collapsing. The plugin's
  `getVisibilityProps` returns `{ display: 'none' }` for `!node.visible` and
  merges it with the rest at `index.ts:203`; this repo does the same at
  `style.rs:274`.
- **A variant prop is whatever the set calls it.** Here it is
  `<Header status="landing" />` and `<Footer status="landing" />` — not
  `property1`, and the same value at all three widths. That is a different
  situation from the `notice` footer noted below: there the widths differ and
  the plugin picks one, here they genuinely agree, so nothing is being papered
  over.

### `button/` — a component set, for what a call site may not pass

Node `582:2137`. Not a screen: a four-dimension component set (`size`,
`varient`, and the boolean `leftIcon` / `rightIcon`) that also carries an
`effect` dimension.

| File | The plugin's tab |
|---|---|
| `pure.tsx` | Pure Code, one variant flattened |
| `usage.tsx` | Usage — how a call site writes it |
| `components.tsx` | Button - Components |

**`effect` is not a prop.** `ButtonProps` does not declare it. The dimension
exists so the *definition* can fold its variants into `_hover` and `_active`
blocks, which is where all five of the set's hover colours live. A call site has
no interaction state to pass, so `usage.tsx` writes
`<Button leftIcon rightIcon size="lg" varient="primary" />` and no `effect`.
`viewport` is the same for widths. Emitting either at a call site asks a
component for a prop it does not have; this repo used to write
`<Tab effect="selected" />` in merged screens and no longer does.

Three more things it settles, none of which the two screens show:

- **A boolean variant is a bare attribute**, and its subtree is guarded by the
  dimensions that admit it:
  `{leftIcon && (size === "lg" || size === "md" || size === "sm") && …}` — the
  `tag` size has no icon, so the guard is not decoration.
- **Per-variant maps nest.** Where a value depends on two dimensions, the inner
  map is indexed inside the outer one:
  `px={{ lg: { ghost: "10px", … }[varient], tag: "10px" }[size]}`.
- **Text content is a variant too**, not only the props around it:
  `{{ lg: "buttonLg", md: "button", tag: "Tag" }[size]}`. And `typography` alone
  is cast — `({…} as const)[size]` — because it is a token name rather than a
  free string.

### `keyframes/` — a timed Smart Animate, for what a chain of frames becomes

`devup-Test`'s `458:2021`, the frame `1` in the Section `애니메이션 Test`: a
loading spinner drawn as eight frames, each Smart-Animating to the next after
a timeout and the last back to the first. The plugin follows the chain and
writes what moves from one frame to the next as CSS keyframes.

| file | plugin output | what it is |
|---|---|---|
| `pure.tsx` | Pure Code | the first frame, with the chain as `animationName={keyframes({...})}` |

What it settles: the keyframes are the *differences* between consecutive
frames — `0%` is the first frame seen from the second, each step is the next
frame seen from the previous, a step that repeats the one before it is left
out, and a loop ends at `100%` back at the start with the duration counting
the return (`7 × 0.2s + 0.2s = 1.6s`). A timeout under 10ms is no delay. The
answer writes no size on the eight 12px frames that hold the dots; this repo
keeps `boxSize="12px"` on them, because without it each frame collapses to
its dot and the dot lands up to 5px off — recorded as a deliberate difference
in `crates/devup-mcp-devup-ui/tests/keyframes_screen.rs`.


### `grid/` — six pictures in a grid

`devup-Test`'s `429:1966`, `Frame 501`: six frames, each an image, in a
frame Figma reads as a 3×3 grid. Pure Code only, in `pure.tsx`.

What it settles: a grid is a `Grid` with `gridTemplateColumns` /
`gridTemplateRows` as `repeat(n, 1fr)`, and each child names its cell as
`gridColumn="c / span 1"` and `gridRow="r / span 1"` — except the first,
which sits where the flow would put it and says nothing. This repo writes it
line for line.

### `report/` — a translucent gradient between tokens

`devup-Test`'s `446:1971`, `Section5 : report`: a section whose backdrop is a
50%-opacity linear gradient between two colour variables over a solid token,
three cards with the same kind of gradient at 80% and a 50% stop, a heading
with a gradient fill clipped to its text, and an illustration — a rotated
group — pinned below its frame. Pure Code only, in `pure.tsx`.

What it settles, and what it found: a gradient stop bound to a variable whose
alpha, times the paint's, is under 1 is written as
`color-mix(in srgb, $token, transparent N%)`, `N` being the lost part —
`transparent 50%` on the backdrop, `20%` and `60%` on the cards. This repo
wrote the bare token, dropping the alpha, until this answer; the plugin's
tests for it were filed under a generic Rust test that never exercised it.
And a group has no constraints of its own, so the plugin positions it by its
first child's: this illustration's children are pinned to the bottom, so it
is `bottom="-284.8px"`, where this repo had read the group alone and written
`top`. Both are fixed and the answer matches line for line, with one
difference on purpose: the illustration keeps its height, as every folded
asset does here (see `keyframes/`).

### Doubtful, on the capture

`components.tsx` gives `disabled` an entry in `_hover` and `_active`:

```tsx
_hover={{ "bg": { …, disabled: "$errorDark", … }[varient] }}
_active={{ "borderRadius": varient === 'disabled' && "6px" }}
```

The set has no such variant. Its 49 components carry `varient=disabled` at four
sizes and only at `effect=default` — there is no disabled hover or active
anywhere in the file, and the capture accounts for every one of them.

The plugin's own code does not produce this either, which is worth saying
because it was the obvious explanation and it is wrong.
`mergePropsAcrossComposites` gives a combination that lacks a pseudo-selector
an empty object, so its inner props come out `null`; the nulls are then
dropped before the map is built, and a combination that was never drawn cannot
reach the output. Whatever produced these entries, it was not that.

What is left is that the answer and the file disagree about the design rather
than about the rules — the answer was taken before the disabled hover variants
were removed, or from another state of the file. The values fit that reading:
`$errorDark` is what `error` hovers to and `6px` is `tag`'s radius, which is
what a neighbouring variant would have contributed. This repo leaves them out,
because the file it is given does not contain them.

The whole definition is compared line for line in
`crates/devup-mcp-devup-ui/tests/button_answer.rs`, and two more of its lines
are the file's and not the answer's:

- **The icons are coloured per variant.** The answer gives both icons
  `bg="$gray400"` at every variant. The capture gives the icon instances a
  white fill on `primary` and `error`, `text` on `white` and `ghost`, and
  `gray400` on `disabled` alone — the same map the answer itself writes for
  the label's `color`. `$gray400` is the icon *component's* colour; the
  instances override it, and this repo reads the instances.
- **The right icon has an aspect ratio where the file gives it one.** The
  `Arrow` instances carry `targetAspectRatio` 20×20 at `md`, `sm` and
  `lg`/`ghost` and none at the other `lg`s, which this repo writes as
  `aspectRatio={{lg: varient === 'ghost' && "1", md: "1", sm: "1"}[size]}`;
  the answer drops it, as the plugin's definition and component-referencing
  outputs drop `targetAspectRatio` everywhere (see `notice/`).

And two where the two outputs say the same thing differently, kept as they
are on purpose: a map's keys follow the order the set declares its options in
(`primary, white, ghost, disabled, error`, the order of the `varient` type in
the interface) where the plugin's follow the layer order of the set's
children; and a hover value is written where hovering changes it (`gap` on
`md`/`ghost`, `8px` at rest) where the plugin writes it on every `md`, four
of which already rest at `10px`.

### `devup-ui-landing/` — a real shipped page, in a file of its own

Not from `devup-Test`. This is the devup-ui.com landing page, in
`JVj6yCOUnF45JQAPvXLA4p`, and it is here because it is the first answer with
a *third* account to check against: the design, the plugin's output, and the
site that is actually deployed from `dev-five-git/devup-ui`. Three frames,
and the plugin's Pure Code and component-referencing outputs at each:

| Width | Frame | Drawn | Files |
|---|---|---|---|
| mobile | `833:3640` | 360×2955 | `pure-mobile.tsx` / `mobile.tsx` |
| tablet | `833:3322` | 992×2964 | `pure-tablet.tsx` / `tablet.tsx` |
| PC | `832:2975` | 1920×3084 | `pure-pc.tsx` / `pc.tsx` |

No test compares these yet. The screens are rendered against Figma's own
PNGs by `harness/render` instead, which is what has been reading them, and
what found everything below.

**Only `pure-pc.tsx` is the answer for the frame beside it.** The other five
were captured from an earlier state of the file - `pure-mobile.tsx` has no
join-us section at all, where the frame does - and are still to be replaced.
Until they are, a difference against them is a question and not a verdict,
and the render is the judge.

What the page has settled so far, all of them things the plugin gets wrong
too:

- **A newline in `characters` is a line break.** The headline's `characters`
  is `'Zero Config\nZero FOUC\nZero Runtime\nCSS in JS Preprocessor'`, four
  lines. The answer folds the first two breaks into `{" "}` and keeps only
  the third as `<br />`, so the heading runs `Zero Config Zero FOUC Zero
  Runtime` on one line; the sub-heading loses its break the same way. This
  repo writes all three, which is what Figma draws.
- **A hugging child is not stretched.** The `Get started` button is 247px
  wide in the 1360px column that holds it, and both this repo and the answer
  drew it 1360px wide — a black bar across the hero — because Figma writes
  its default cross-axis alignment by leaving the field out and CSS reads
  that absence as `stretch`.
- **A child Figma will not shrink must say so.** The comparison row is seven
  240px cards in a 912px frame. Figma keeps them at 240 and lets the row
  spill past the frame, which clips it; CSS squeezed all seven into 912,
  wrapped their labels, and the row came out 58px taller, carrying the rest
  of the page down with it. The tablet went from 11.60% to 5.55% once the
  fixed children stopped shrinking.
- **Three of its 215 assets could not be fetched**, which is what found all
  three transport bugs behind them: the 1232×1232 hero is 950KB, past what
  Figma returns as an attachment; a footer logo sits in a frame that holds
  the picture without carrying the fill, so it was asked for by a fill index
  it does not have; and two icons sit entirely outside a panel that clips, so
  they draw nothing and Figma will not export them at all. None of this is
  visible in the code the plugin writes — it names the files and stops — so
  only fetching them showed it.

Still open, in the order they cost pixels:

- **The mobile is at 11.56%**, from many small height errors that accumulate
  to about 20px down the page rather than from any single one.
- **The hero picture's placement.** This repo sizes a positioned asset by the
  box its export frames where the answer writes the node's own; which is
  right has not been settled against the render.
- **Two icons the code still points at** are the clipped-away ones above. The
  manifest now says up front that they cannot be exported, but the code goes
  on naming them, so the render draws a mask with no file behind it.

## What `popup/` settles about the array

**A prop that stops has to say so.** `null` in a devup-ui array does not mean
"no value" — it means "whatever the slot before it said". So a prop cannot be
dropped at a wider width; the narrower value would be inherited. The reference
writes `"initial"`:

```tsx
pl={["36.5px", "initial"]}                    // mobile only
px={[null, "184px", null, null, "initial"]}   // tablet only
```

The two differ in a way worth reading. `pl`'s slot 0 is a value because mobile
sets it; `px`'s slot 0 is `null` because nothing is in effect yet and there is
nothing to clear. `"initial"` is only spent where something would otherwise be
inherited.

**A width's slot comes from how wide it is, not what it is called.** The
plugin's `getBreakpointByWidth` puts a frame in the first band it fits:

| Slot | 0 | 1 | 2 | 3 | 4 |
|---|---|---|---|---|---|
| name | mobile | sm | tablet | lg | pc |
| width | ≤ 480 | ≤ 768 | ≤ 992 | ≤ 1280 | rest |

Both screens have frames named `mobile` / `tablet` / `desktop`, and they land
differently. `notice` is drawn at 360 / 992 / 1920 and uses slots 0 / 2 / 4;
`popup` is drawn at 390 / 768 and its `tablet` takes slot **1**, the band
`notice` skips. Reading the name and assuming slot 2 would put every `popup`
value a band too wide — which is what the arrays in its answer show, and what
the frames' own widths confirm.

**The element itself can merge.** `popup`'s root is a `VStack` at two widths and
a `Flex` at the third. The merge does not keep both — it writes one `VStack` and
demotes the difference to a prop:

```tsx
<VStack flexDir={["column", null, null, null, "row"]} ... >
```

**Props are unioned, not reconciled.** Mobile sets `pl` and `pr`, tablet sets
`px`, desktop sets neither. All three survive as three separate arrays; nothing
tries to rewrite `pl`/`pr` into `px`. Every prop any width mentions gets its own
array covering all widths.

## What the definitions settle

`docs/responsive-merge-rules.md` argued from one example that a definitions
output cannot be recovered by diffing the other two. The file bears that out at
a scale worth writing down. Across eight components it declares five prop
unions naming 67 options, of which the screen exercises four:

| Component | Options | Reachable from this screen |
|---|---|---|
| `Icons` | 56 | none — the only call site is commented out |
| `Header` | 4 | `transparent` |
| `Footer` | 3 | `desktop` |
| `HeaderItem` | 2 | `기본` |
| `ThemeButton` | 2 | `Light` |

It also carries six `_hover` / `_active` / `_selected` blocks, on
`LanguageButton`, `Tab` and `Pagination`. Not one of them appears anywhere in
`notice/pure.tsx` or `notice/with-components.tsx`, because a call site has no syntax for
them. Interaction state is only ever stated in a definition.

## Pure Code is not the same tree under different names

The two single-width outputs differ in shape, not only in whether an instance
is spelled out. Where `notice/with-components.tsx` writes

```tsx
<Box left="0px" pos="absolute" top="0px" w="100%">
    <Header property1="transparent" />
</Box>
```

`notice/pure.tsx` writes one element, and its props are exactly the wrapper's four
plus the four the `Header` root resolves to at `property1="transparent"`:

```tsx
<Flex alignItems="center" justifyContent="space-between" left="0px"
      overflow="hidden" pos="absolute" px="20px" top="0px" w="100%">
```

So the wrapper is not decoration. An instance node carries its own placement,
and `<Header />` has nowhere to put it, so the referencing output has to add a
`Box` that the expanded output does not need. Generating one from the other is
a structural change, not a substitution.

## Known doubtful, by the author

*The rest of this file is about `notice/`, and unqualified filenames below mean
that directory.*

- **`<Footer property1="desktop" />` in `notice/responsive.tsx`.** It stays `desktop` at
  every width, though `FooterProps` admits `'mobile'` and `'tablet'` and
  `notice/components.tsx` lays out all three. Component props do not go responsive —
  passing an array there does nothing — and the author reads this as the plugin
  not having implemented it rather than as intended. Do not match this.

## Doubtful on the evidence

- **Pure Code is the odd one out — and on the colour, it is the one that is
  right.** Where the three outputs describe the same node, `pure.tsx` differs
  from both others. The search icon — one `Icons` instance at
  `Property1="search"` — reads:

  | | `pure.tsx` | `with-components.tsx` and `responsive.tsx` |
  |---|---|---|
  | `bg` | `$primary` | `$text` |
  | `maskImage` | `icons.svg` | `Property 1=search.svg` |
  | `aspectRatio` | `"1"` | absent |

  This file used to conclude that Pure Code was outvoted. The capture says
  otherwise on the first row: the `Union` vector inside every `icons` instance
  (`I422:6887;13:1876`, and the two at the other widths) has its fill bound to
  `VariableID:422:7203`, whose name in the collected variables is `primary`.
  So `$primary` is the file, and the two outputs that agree on `$text` agree
  on something the file does not contain. The same goes for the banner logos,
  which `responsive.tsx` writes as `bg="$text"` with no opacity: each is a
  `Logo` instance at opacity 0.1 whose one vector has no fill and a white
  stroke, which the plugin's own `analyzeOwnSameColor` reads as `#FFF`. Two
  outputs agreeing is not two accounts when they were written from another
  state of the file than the third.

  On the second row the reading stands: `icons.svg` is the component set's
  name and `Property 1=search.svg` is the selected variant's. This repo names
  the asset after the instance's own layer, on purpose, because every set with
  a `search` variant would otherwise share one file. On the third, `pure.tsx`
  carries `aspectRatio` 8 times and `with-components.tsx` never. It is a real
  property — the footer logo is `220px` by `16px`, which is 13.75, yet both
  outputs that mention it say `aspectRatio="14"`, so it is Figma's locked
  `targetAspectRatio` and not a ratio computed from the box. One path reads
  it, the others drop it. Emit it on all of them.

- **The banner is kept twice.** Its shape differs between widths because two
  logos are wrapped in a frame on desktop and left loose on mobile — one intent
  grouped two ways, which is drift in the design file. The plugin's answer is
  reasonable given that input, but a design whose widths agree in shape should
  never produce it, so this is not a pattern to reproduce for its own sake. See
  `docs/responsive-merge-rules.md`.

## Looks wrong, is not

- **`bg="$text"` in a definition, `bg="#FFF"` at the call site.** The header
  logo and the selected menu underline read as tokens in `components.tsx` and
  as white in `pure.tsx`, which looks like Pure Code resolving tokens away. It
  is not: `pure.tsx` emits `$text` eight times and `$primary` seven. The header
  sits on the purple banner, and the screen overrides those fills to white on
  the instance. Both outputs are reporting what their own node says.

## Not doubted

Everything measured against the pinned corpus agreed with what this repo emits:
angles, mask positions, image folders, border shorthand order, omitted canvas
sizes, blend flattening. Where these files and the corpus say the same thing,
that is two independent accounts, and the bar to differ from them is high.
