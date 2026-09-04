# What the plugin answered

Not "the correct output". These are the four things `devup-figma-plugin` returned
when asked for one frame — the `desktop` width (`422:6865`) of the `notice`
screen in `devup-Test` — and they are kept because they are the only account of
how it behaves on a real screen, not because they are known to be right.

The distinction matters. The plugin is the reference this repo aims to match or
beat, and its author does not vouch for every line here. Treat a difference as a
question, the same way a difference against the pinned corpus is a question, and
say which way it was settled.

## The files

| File | The plugin's tab | What it shows |
|---|---|---|
| `pure.tsx` | Pure Code | the frame with every instance expanded to primitives |
| `with-components.tsx` | desktop | the same frame with instances left as `<Header />` |
| `components.tsx` | desktop - Components | the definitions of those components |
| `responsive.tsx` | notice - Responsive | all three widths merged |

Only `responsive.tsx` carries `display` arrays. The other three describe one
width.

## Known doubtful, by the author

- **`<Footer property1="desktop" />` in `responsive.tsx`.** It stays `desktop` at
  every width, though `FooterProps` admits `'mobile'` and `'tablet'` and
  `components.tsx` lays out all three. Component props do not go responsive —
  passing an array there does nothing — and the author reads this as the plugin
  not having implemented it rather than as intended. Do not match this.

## Doubtful on the evidence

- **The banner is kept twice.** Its shape differs between widths because two
  logos are wrapped in a frame on desktop and left loose on mobile — one intent
  grouped two ways, which is drift in the design file. The plugin's answer is
  reasonable given that input, but a design whose widths agree in shape should
  never produce it, so this is not a pattern to reproduce for its own sake. See
  `docs/responsive-merge-rules.md`.

## Not doubted

Everything measured against the pinned corpus agreed with what this repo emits:
angles, mask positions, image folders, border shorthand order, omitted canvas
sizes, blend flattening. Where these files and the corpus say the same thing,
that is two independent accounts, and the bar to differ from them is high.
