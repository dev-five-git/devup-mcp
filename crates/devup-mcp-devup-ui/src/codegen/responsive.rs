//! Lining up the same screen drawn at several widths.
//!
//! A responsive screen is three sibling frames in a Section, named for the
//! width they are, and the conversion wants them as one tree whose differing
//! values became arrays. That is only possible where the trees agree in shape,
//! and this module is the part that finds out: it pairs the roots up by name,
//! walks them together, and names every place they part company.
//!
//! Shape divergence is not the interesting case — it is the cost of one. Widths
//! of the same screen are meant to be the same tree three times, so a place
//! where they are not is usually a slip in the file, and the export can only
//! carry it by keeping both copies and showing each at its own widths. Saying
//! where that happened is the point of reporting it: silently keeping both
//! looks like success and hides the thing worth fixing.
//!
//! Keeping both is not a second code path, though, which is the thing reading
//! the output does not tell you. A width that does not draw a node is handed a
//! hidden copy of one that does, and the copies go through the ordinary merge;
//! the `display` array falls out of it like any other prop. `divergences` is
//! therefore a report, not a switch.
//!
//! The rules here are ported from `devup-figma-plugin` at the commit this
//! repo's corpus pins — `src/codegen/responsive/`.

// Source sections share this module to preserve privacy and test names.
include!("responsive/breakpoint_merge.rs");
include!("responsive/child_alignment.rs");
include!("responsive/width_label.rs");
include!("responsive/merge_content.rs");
include!("responsive/impl.rs");
