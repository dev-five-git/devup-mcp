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
call site may not pass. `about` has no directory, for the reason its own section
gives.

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

Node `422:3798`. Its Pure Code at `desktop` and its Responsive output were both
read, and unlike the three directories above **no file is kept for it**. A
reference is only useful as a judge if it is byte-exact, and this one reached
this repo through a chat window rather than a capture; a hand-copied 1,400-line
file would be a judge that cannot be trusted, which is worse than none. What was
checkable was checked and locked instead, in
`responsive_merge.rs::the_about_screen_needs_every_slot`.

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

- **Pure Code is the odd one out, and it is outvoted.** Where the three
  outputs describe the same node, `pure.tsx` differs from both others. The
  search icon — one `Icons` instance at `Property1="search"` — reads:

  | | `pure.tsx` | `with-components.tsx` and `responsive.tsx` |
  |---|---|---|
  | `bg` | `$primary` | `$text` |
  | `maskImage` | `icons.svg` | `Property 1=search.svg` |
  | `aspectRatio` | `"1"` | absent |

  `icons.svg` is the component set's name and `Property 1=search.svg` is the
  selected variant's, so on that column Pure Code is not merely different but
  wrong. The `aspectRatio` split is the same story everywhere: `pure.tsx`
  carries it 8 times, `with-components.tsx` never. It is a real property —
  the footer logo is `220px` by `16px`, which is 13.75, yet both outputs that
  mention it say `aspectRatio="14"`, so it is Figma's locked
  `targetAspectRatio` and not a ratio computed from the box. One path reads
  it, the others drop it. Emit it on all of them, and do not take `pure.tsx`
  as the account of a node when the other two agree against it.

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
