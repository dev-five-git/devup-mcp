//! Each motion rule against entrances written with Devup UI keyframes, with
//! the shapes that must stay silent. The framer-motion forms of the same
//! rules are in `motion_quality.rs`.
use super::tests::rules;
use super::*;

const RISE: &str = "const rise = keyframes({ from: { opacity: 0, transform: 'translateY(16px)' }, to: { opacity: 1, transform: 'translateY(0)' } });";

fn with_rise(body: &str) -> String {
    format!("{RISE}\n{body}")
}

/// The form the guide recommends: nothing to report.
const RECOMMENDED: &str = r#"const C = () => <Box as="section" bg="$surface"><Box animationName={rise} animationTimingFunction="ease-out" animationFillMode="both" animationTimeline="view()" animationRange="entry 0% entry min(100%, 240px)" _motionReduce={{ animation: 'none' }} _focusWithin={{ animation: 'none' }} selectors={{ '&[data-reveal="pending"]': { animationName: 'none', opacity: 0 }, '&[data-reveal="in"]': { animationTimeline: 'auto', animationDuration: '400ms' } }}><Title /></Box></Box>"#;

#[test]
fn the_recommended_keyframes_reveal_is_clean() {
    let source = with_rise(RECOMMENDED);
    assert_eq!(findings(&source).into_iter().map(|finding| finding.rule).collect::<Vec<_>>(), Vec::<&str>::new());
    assert!(reduced_motion_finding(&source).is_none());
}

#[test]
fn keyframes_content_that_rests_hidden_is_hidden_server_markup() {
    for body in [
        // Resting opacity 0: the animation is the only thing that shows it.
        r#"const C = () => <Box opacity={0} animation={`${rise} 400ms ease-out forwards`}>Copy</Box>"#,
        r#"const C = () => <Box opacity="0" animationName={rise} animationDuration="400ms" animationFillMode="forwards">Copy</Box>"#,
        r#"const hidden = css({ opacity: 0, animationName: rise, animationDuration: '400ms', animationFillMode: 'forwards' })"#,
        // Hidden until a script sets a marker.
        r#"const C = () => <Box opacity={0} transition="opacity .4s" selectors={{ '&[data-visible="true"]': { opacity: 1 } }}>Copy</Box>"#,
        r#"const C = () => <Box visibility="hidden" selectors={{ '&.is-visible': { visibility: 'visible' } }}>Copy</Box>"#,
        // Paused on the hidden first frame until a script resumes it.
        r#"const C = () => <Box animation={`${rise} 400ms ease-out both`} animationPlayState="paused">Copy</Box>"#,
        // An inline keyframes() call is read the same way.
        r#"const C = () => <Box opacity={0} animationName={keyframes({ '0%': { opacity: 0 }, '100%': { opacity: 1 } })} animationDuration="300ms">Copy</Box>"#,
    ] {
        assert_eq!(rules(&with_rise(body)), ["ssr-hidden-reveal"], "{body}");
    }
    for body in [
        // The hidden start lives only in the keyframes.
        r#"const C = () => <Box animation={`${rise} 400ms ease-out both`}>Copy</Box>"#,
        // Hidden only once a script has marked it as still below the viewport.
        r#"const C = () => <Box animationName={rise} animationFillMode="both" animationDuration="400ms" selectors={{ '&[data-reveal="pending"]': { opacity: 0 } }}>Copy</Box>"#,
        // A hover overlay is not an entrance, and no script marker shows it.
        r#"const C = () => <Box opacity={0} _hover={{ opacity: 1 }} transition="opacity .2s">Overlay</Box>"#,
        // Client-mounted: never part of server markup.
        r#"const C = () => open && <Box opacity={0} animation={`${rise} 400ms ease-out forwards`}>Toast</Box>"#,
        // A paused spinner does not hide anything.
        r#"const spin = keyframes({ from: { transform: 'rotate(0)' }, to: { transform: 'rotate(360deg)' } }); const C = () => <Box animation={`${spin} 1s linear infinite`} animationPlayState="paused" />"#,
        // A value built elsewhere cannot be read from here.
        r#"const C = () => <Box opacity={start} animationName={rise} animationDuration="400ms">Copy</Box>"#,
    ] {
        assert_eq!(rules(&with_rise(body)), Vec::<&str>::new(), "{body}");
    }
}

#[test]
fn a_view_timeline_reveal_must_end_inside_the_entry() {
    for body in [
        // No range: the fade spans the whole pass through the viewport.
        r#"const C = () => <Box animationName={rise} animationFillMode="both" animationTimeline="view()">Copy</Box>"#,
        r#"const C = () => <Box animationName={rise} animationTimeline="view()" animationRange="cover">Copy</Box>"#,
        r#"const C = () => <Box animationName={rise} animationTimeline="view()" animationRange="entry 0% exit 100%">Copy</Box>"#,
        r#"const C = () => <Box animationName={rise} animationTimeline="view()" animationRange="entry 0% cover 80%">Copy</Box>"#,
        r#"const reveal = css({ animationName: rise, animationTimeline: 'view()', animationRangeEnd: 'exit 0%' })"#,
    ] {
        assert_eq!(rules(&with_rise(body)), ["reveal-replays"], "{body}");
    }
    for body in [
        r#"const C = () => <Box animationName={rise} animationTimeline="view()" animationRange="entry 0% entry 100%">Copy</Box>"#,
        r#"const C = () => <Box animationName={rise} animationTimeline="view()" animationRange="entry">Copy</Box>"#,
        r#"const C = () => <Box animationName={rise} animationTimeline="view()" animationRange="entry 0% cover 30%">Copy</Box>"#,
        r#"const reveal = css({ animationName: rise, animationTimeline: 'view()', animationRangeEnd: 'entry 100%' })"#,
        // Built elsewhere: unknown, so not reported.
        r#"const C = () => <Box animationName={rise} animationTimeline="view()" animationRange={range}>Copy</Box>"#,
        // A timed, one-shot entrance has no range to state.
        r#"const C = () => <Box animation={`${rise} 400ms ease-out both`}>Copy</Box>"#,
        // A page progress bar is not an entrance.
        r#"const grow = keyframes({ from: { transform: 'scaleX(0)' }, to: { transform: 'scaleX(1)' } }); const C = () => <Box animationName={grow} animationTimeline="scroll()" />"#,
    ] {
        assert_eq!(rules(&with_rise(body)), Vec::<&str>::new(), "{body}");
    }
}

#[test]
fn a_fallback_observer_marks_once() {
    let mirrors = r#"const observer = new IntersectionObserver(entries => entries.forEach(entry => entry.target.toggleAttribute('data-in', entry.isIntersecting)))"#;
    let found = findings(mirrors);
    assert_eq!(found.iter().map(|finding| (finding.rule, finding.severity)).collect::<Vec<_>>(), [("reveal-replays", Severity::Warning)], "{mirrors}");
    for source in [
        r#"const observer = new IntersectionObserver(entries => entries.forEach(entry => entry.target.classList.toggle('in', entry.isIntersecting)))"#,
        r#"const observer = new IntersectionObserver(function (entries) { for (const entry of entries) entry.target.dataset.reveal = entry.isIntersecting ? 'in' : 'pending' })"#,
    ] {
        assert_eq!(findings(source).iter().map(|finding| (finding.rule, finding.severity)).collect::<Vec<_>>(), [("reveal-replays", Severity::Warning)], "{source}");
    }
    // No stop, but nothing proves it drives an entrance either.
    let tracks = r#"const observer = new IntersectionObserver(entries => entries.forEach(entry => { if (entry.isIntersecting) load(entry.target) }))"#;
    assert_eq!(findings(tracks).iter().map(|finding| (finding.rule, finding.severity)).collect::<Vec<_>>(), [("reveal-replays", Severity::Info)]);
    for source in [
        r#"const observer = new IntersectionObserver(entries => entries.forEach(entry => { if (!entry.isIntersecting) return; entry.target.dataset.reveal = 'in'; observer.unobserve(entry.target) }))"#,
        r#"const observer = new IntersectionObserver((entries, self) => { if (entries.some(entry => entry.isIntersecting)) { start(); self.disconnect() } })"#,
        // A callback declared elsewhere cannot be read from here.
        r#"const observer = new IntersectionObserver(onChange, { threshold: 0.2 })"#,
        r#"const observer = new ResizeObserver(entries => entries.forEach(entry => entry.target.classList.toggle('wide', entry.contentRect.width > 600)))"#,
    ] {
        assert_eq!(rules(source), Vec::<&str>::new(), "{source}");
    }
}

#[test]
fn a_keyframes_reveal_stays_off_the_painted_section() {
    for body in [
        r#"const C = () => <Box as="section" bg="$surface" animationName={rise} animationFillMode="both" animationTimeline="view()" animationRange="entry 0% entry 100%"><Title /></Box>"#,
        r#"const C = () => <Box as="section" bgImage="url(/band.webp)" animation={`${rise} 400ms ease-out both`}><Title /></Box>"#,
        r#"const band = css({ backgroundColor: '$surface', animationName: rise, animationDuration: '400ms', animationFillMode: 'both' })"#,
        // The class paints and the element's own props animate.
        r#"const C = () => <section className={css({ bg: '$surface' })} animationName={rise} animationDuration="400ms" animationFillMode="both" />"#,
        // The class animates and the element's own props paint.
        r#"const C = () => <Box bg="$surface" className={css({ animationName: rise, animationDuration: '400ms', animationFillMode: 'both' })} />"#,
    ] {
        assert_eq!(rules(&with_rise(body)), ["reveal-moves-background"], "{body}");
    }
    for body in [
        // Paint outside, motion inside.
        r#"const C = () => <Box as="section" bg="$surface"><Box animation={`${rise} 400ms ease-out both`}><Title /></Box></Box>"#,
        // A looping loader on a painted element is not an entrance.
        r#"const pulse = keyframes({ from: { opacity: 0 }, to: { opacity: 1 } }); const C = () => <Box bg="$skeleton" animation={`${pulse} 1s ease-in-out infinite alternate`} />"#,
        r#"const spin = keyframes({ from: { transform: 'rotate(0)' }, to: { transform: 'rotate(360deg)' } }); const C = () => <Box bg="$primary" animation={`${spin} 1s linear infinite`} />"#,
        // Keyframes declared in another module cannot be read from here.
        r#"const C = () => <Box bg="$surface" animationName={imported} animationDuration="400ms" />"#,
        r#"const C = () => <Box bg="$surface" transition="background-color .2s" _hover={{ bg: '$surfaceHover' }} />"#,
    ] {
        assert_eq!(rules(&with_rise(body)), Vec::<&str>::new(), "{body}");
    }
}

#[test]
fn a_keyframes_stagger_stays_short() {
    for body in [
        r#"const C = () => items.map((item, index) => <Box key={item.id} animation={`${rise} 400ms ease-out both`} animationDelay={`calc(${index} * 250ms)`} />)"#,
        r#"const C = () => items.map((item, index) => <Box key={item.id} animation={`${rise} 400ms ease-out both`} animationDelay={`${index * 200}ms`} />)"#,
        r#"const C = () => items.map((item, index) => <Box key={item.id} animation={`${rise} 400ms ease-out both`} animationDelay={`${0.3 * index}s`} />)"#,
        r#"const C = () => items.map((item, index) => <Box key={item.id} style={{ '--i': index }} animation={`${rise} 400ms ease-out both`} animationDelay="calc(var(--i) * 0.2s)" />)"#,
        r#"const card = css({ animationName: rise, animationDuration: '400ms', animationFillMode: 'both', animationDelay: 'calc(180ms * var(--i))' })"#,
    ] {
        assert_eq!(rules(&with_rise(body)), ["stagger-too-long"], "{body}");
    }
    for body in [
        r#"const C = () => items.map((item, index) => <Box key={item.id} animation={`${rise} 400ms ease-out both`} animationDelay={`calc(${index} * 60ms)`} />)"#,
        r#"const C = () => items.map((item, index) => <Box key={item.id} animation={`${rise} 400ms ease-out both`} animationDelay={`${index * 60}ms`} />)"#,
        r#"const C = () => <Box style={{ '--i': index }} animation={`${rise} 400ms ease-out both`} animationDelay="calc(var(--i) * 60ms)" />"#,
        // One fixed wait, and arithmetic that is not a per-item step.
        r#"const C = () => <Box animation={`${rise} 400ms ease-out both`} animationDelay="400ms" />"#,
        r#"const C = () => <Box animation={`${rise} 400ms ease-out both`} animationDelay="calc(2 * 300ms)" />"#,
        r#"const C = () => <Box animation={`${rise} 400ms ease-out both`} animationDelay={`${base}ms`} />"#,
    ] {
        assert_eq!(rules(&with_rise(body)), Vec::<&str>::new(), "{body}");
    }
}

#[test]
fn a_keyframes_entrance_needs_reduced_motion_handling() {
    for body in [
        r#"const C = () => <Box animation={`${rise} 400ms ease-out both`}>Copy</Box>"#,
        r#"const C = () => <Box animationName={rise} animationFillMode="both" animationTimeline="view()" animationRange="entry 0% entry 100%">Copy</Box>"#,
        r#"const reveal = css({ animationName: rise, animationDuration: '400ms', animationFillMode: 'both' })"#,
        // A comment or an unused import is not handling.
        "// _motionReduce later\nconst C = () => <Box animation={`${rise} 400ms ease-out both`}>Copy</Box>",
    ] {
        let source = with_rise(body);
        assert_eq!(reduced_motion_finding(&source).map(|finding| finding.rule), Some("motion-without-reduced-motion"), "{body}");
    }
    for body in [
        r#"const C = () => <Box animation={`${rise} 400ms ease-out both`} _motionReduce={{ animation: 'none' }}>Copy</Box>"#,
        r#"const C = () => <Box _motionSafe={{ animation: `${rise} 400ms ease-out both` }}>Copy</Box>"#,
        r#"const reveal = css({ animationName: rise, animationDuration: '400ms', selectors: { '@media (prefers-reduced-motion: reduce)': { animationName: 'none' } } })"#,
        // Not an entrance: a loader, a hover transition, keyframes that only move.
        r#"const spin = keyframes({ from: { transform: 'rotate(0)' }, to: { transform: 'rotate(360deg)' } }); const C = () => <Box animation={`${spin} 1s linear infinite`} />"#,
        r#"const C = () => <Box transition="opacity .2s" _hover={{ opacity: 0.8 }} />"#,
        r#"const nudge = keyframes({ from: { transform: 'translateX(-4px)' }, to: { transform: 'translateX(0)' } }); const C = () => <Box animation={`${nudge} 200ms ease-out`} />"#,
    ] {
        let source = with_rise(body);
        assert!(reduced_motion_finding(&source).is_none(), "{body}");
    }
}

#[test]
fn a_library_entrance_is_noted_once_and_keyframes_are_not() {
    let twice = r#"const C = () => <><motion.div whileInView="visible" viewport={{ once: true }} /><motion.div whileInView="visible" viewport={{ once: true }} /></>"#;
    let found = findings(twice);
    assert_eq!(found.iter().map(|finding| (finding.rule, finding.severity)).collect::<Vec<_>>(), [("entrance-uses-motion-library", Severity::Info)]);
    assert!(found[0].message.contains("static CSS"), "{}", found[0].message);
    for source in [
        with_rise(RECOMMENDED),
        // Enter/exit state motion and gestures are what a library is for.
        r#"const C = () => <AnimatePresence>{open && <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} />}</AnimatePresence>"#.to_owned(),
        r#"const C = () => <motion.button whileHover={{ scale: 1.02 }} />"#.to_owned(),
    ] {
        assert!(findings(&source).iter().all(|finding| finding.rule != "entrance-uses-motion-library"), "{source}");
    }
}

#[test]
fn what_the_reader_uses_at_once_gets_no_entrance() {
    for body in [
        r#"const C = () => <Box as="nav" animation={`${rise} 400ms ease-out both`}><Links /></Box>"#,
        r#"const C = () => <footer animationName={rise} animationFillMode="both" animationTimeline="view()" animationRange="entry"><Legal /></footer>"#,
        r#"const C = () => <Box as="form" className={css({ animationName: rise, animationDuration: '400ms', animationFillMode: 'both' })}><Fields /></Box>"#,
        r#"const C = () => <Text as="p" animation={`${rise} 400ms ease-out both`}>Body copy</Text>"#,
        // The same roles through a motion library.
        r#"const C = () => <motion.nav whileInView="visible" viewport={{ once: true }} />"#,
        r#"const C = () => <Box as="footer" whileInView="visible" viewport={{ once: true }} />"#,
    ] {
        assert_eq!(rules(&with_rise(body)), ["entrance-on-static-content"], "{body}");
    }
    for body in [
        r#"const C = () => <Text as="h2" animation={`${rise} 400ms ease-out both`}>Title</Text>"#,
        r#"const C = () => <button animation={`${rise} 400ms ease-out both`}>Go</button>"#,
        r#"const C = () => <motion.a whileInView="visible" viewport={{ once: true }} href="/more">More</motion.a>"#,
    ] {
        let found = findings(&with_rise(body));
        assert!(found.iter().any(|finding| finding.rule == "entrance-per-element" && finding.severity == Severity::Info), "{body}");
        assert!(found.iter().all(|finding| finding.severity == Severity::Info), "{body}");
    }
    for body in [
        // A content group is what an entrance is for.
        r#"const C = () => <Box as="section"><Box animation={`${rise} 400ms ease-out both`}><Title /></Box></Box>"#,
        r#"const C = () => <motion.div whileInView="visible" viewport={{ once: true }} />"#,
        // Static roles without an entrance: a transition, a loader, nothing at all.
        r#"const C = () => <Box as="nav" transition="opacity .2s" _hover={{ opacity: 0.8 }} />"#,
        r#"const spin = keyframes({ from: { transform: 'rotate(0)' }, to: { transform: 'rotate(360deg)' } }); const C = () => <button animation={`${spin} 1s linear infinite`} />"#,
        r#"const C = () => <footer bg="$surface"><Legal /></footer>"#,
        // Exit/enter state motion of a client-mounted element.
        r#"const C = () => <AnimatePresence>{open && <motion.nav initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} />}</AnimatePresence>"#,
    ] {
        assert_eq!(rules(&with_rise(body)), Vec::<&str>::new(), "{body}");
    }
}
