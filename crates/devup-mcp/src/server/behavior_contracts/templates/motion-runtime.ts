export type MotionBinding = {
  kind: 'marquee' | 'count-up' | 'progress-step-cycle' | 'sliding-route-highlight' | 'section-entrance';
  target: string;
  track?: string;
  group?: string;
  links?: string;
  indicator?: string;
  steps?: string;
  panels?: string;
  progress?: string;
  progressTrack?: string;
  value?: number;
  precision?: number;
  unit?: string;
  prefix?: string;
  locale?: string;
  durationMs?: number;
  pixelsPerSecond?: number;
  distancePx?: number;
  respectReducedMotion?: boolean;
};

const reduced = (binding: MotionBinding) => binding.respectReducedMotion !== false && matchMedia('(prefers-reduced-motion: reduce)').matches;
const elements = (root: ParentNode, selector?: string) => selector ? [...root.querySelectorAll<HTMLElement>(selector)] : [];
const only = (root: ParentNode, selector?: string) => {
  const found = elements(root, selector);
  if (found.length !== 1) throw new Error(`Motion target must match exactly once: ${selector}`);
  return found[0];
};
const duration = (binding: MotionBinding, fallback: number) => Math.max(16, binding.durationMs ?? fallback);
const visible = (node: HTMLElement) => node.getClientRects().length > 0 && getComputedStyle(node).visibility !== 'hidden';
const sourceGroups = (track: ParentNode, selector?: string) => elements(track, selector).filter(node => !(node.inert && node.getAttribute('aria-hidden') === 'true'));
function trackedAnimation(node: HTMLElement, frames: Keyframe[], timing: KeyframeAnimationOptions, running: Set<Animation>) {
  const animation = node.animate(frames, timing); running.add(animation);
  void animation.finished.then(() => running.delete(animation), () => running.delete(animation));
  return animation;
}

function countUp(root: ParentNode, binding: MotionBinding) {
  const node = only(root, binding.target), final = node.textContent ?? '';
  const value = binding.value;
  if (typeof value !== 'number' || !Number.isFinite(value)) throw new Error('CountUp needs a finite bound value');
  const precision = Math.min(12, Math.max(0, binding.precision ?? 0));
  const format = (number: number) => (binding.prefix ?? '') + number.toLocaleString(binding.locale ?? 'en-US', { minimumFractionDigits: precision, maximumFractionDigits: precision }) + (binding.unit ?? '');
  const finalText = format(value), originalLabel = node.getAttribute('aria-label');
  node.setAttribute('aria-label', finalText);
  // The hidden final span reserves the actual responsive font/width. It is
  // presentation only; the accessible value stays final throughout animation.
  const reserve = document.createElement('span'), digits = document.createElement('span');
  reserve.textContent = finalText; reserve.setAttribute('aria-hidden', 'true');
  reserve.style.visibility = 'hidden'; reserve.style.gridArea = '1 / 1';
  digits.textContent = finalText; digits.setAttribute('aria-hidden', 'true'); digits.style.gridArea = '1 / 1';
  const originalDisplay = node.style.display;
  const content = [...node.childNodes]; node.replaceChildren(reserve, digits); node.style.display = 'inline-grid';
  let frame = 0, started = false, disposed = false;
  const observer = new IntersectionObserver(entries => {
    if (started || !entries.some(entry => entry.isIntersecting) || !visible(node)) return;
    started = true; observer.disconnect();
    if (reduced(binding)) return;
    digits.textContent = format(0);
    const start = performance.now(), ms = duration(binding, 1600);
    const tick = (now: number) => {
      if (disposed) return;
      const t = Math.min(1, (now - start) / ms);
      digits.textContent = t === 1 ? finalText : format(value * (1 - (1 - t) ** 3));
      if (t < 1) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
  });
  observer.observe(node);
  return () => {
    disposed = true; cancelAnimationFrame(frame); observer.disconnect();
    node.replaceChildren(...content); node.style.display = originalDisplay;
    if (originalLabel === null) node.removeAttribute('aria-label'); else node.setAttribute('aria-label', originalLabel);
    if (!content.length) node.textContent = final;
  };
}

function marquee(root: ParentNode, binding: MotionBinding) {
  const host = only(root, binding.target), track = only(host, binding.track), groups = sourceGroups(track, binding.group);
  if (groups.length !== 1) throw new Error('Marquee needs exactly one original group');
  const group = groups[0];
  const originals = new Map<HTMLElement, string | null>([host, track].map(node => [node, node.getAttribute('style')]));
  let animation: Animation | undefined, copies: HTMLElement[] = [], disposed = false;
  const rebuild = () => {
    if (disposed || !visible(group)) return;
    const previous = animation?.effect?.getTiming().duration;
    const phase = typeof previous === 'number' && previous > 0 ? (Number(animation?.currentTime ?? 0) % previous) / previous : 0;
    animation?.cancel(); copies.forEach(copy => copy.remove()); copies = [];
    host.style.overflow = 'hidden'; track.style.display = 'flex'; track.style.flexWrap = 'nowrap'; track.style.width = 'max-content';
    if (group.style.flexShrink !== '0') group.style.flexShrink = '0';
    const copy = () => {
      const node = group.cloneNode(true) as HTMLElement;
      node.setAttribute('aria-hidden', 'true'); node.inert = true;
      [node, ...node.querySelectorAll<HTMLElement>('[id]')].forEach(child => child.removeAttribute('id'));
      track.append(node); copies.push(node); return node;
    };
    const second = copy(), period = second.getBoundingClientRect().x - group.getBoundingClientRect().x;
    if (!(period > 0)) { changes.takeRecords(); return; }
    for (let i = 1; i < Math.ceil(host.clientWidth / period) + 1; i += 1) copy();
    if (reduced(binding)) { changes.takeRecords(); return; }
    const speed = Math.max(1, binding.pixelsPerSecond ?? 55), ms = period / speed * 1000;
    animation = track.animate([{ transform: 'translateX(0)' }, { transform: `translateX(${-period}px)` }], { duration: ms, iterations: Infinity, easing: 'linear' });
    animation.currentTime = phase * ms;
    changes.takeRecords();
  };
  // Observe only the source group. Repeated siblings and animation frame
  // mutations cannot retrigger this observer and restart the marquee.
  const changes = new MutationObserver(rebuild);
  changes.observe(group, { childList: true, subtree: true, characterData: true, attributes: true });
  const resize = new ResizeObserver(rebuild); resize.observe(host); resize.observe(group);
  const originalGroupStyle = group.getAttribute('style');
  rebuild(); void document.fonts?.ready.then(rebuild);
  return () => {
    disposed = true; animation?.cancel(); resize.disconnect(); changes.disconnect(); copies.forEach(copy => copy.remove());
    for (const [node, style] of originals) { if (style === null) node.removeAttribute('style'); else node.setAttribute('style', style); }
    if (originalGroupStyle === null) group.removeAttribute('style'); else group.setAttribute('style', originalGroupStyle);
  };
}

function stepCycle(root: ParentNode, binding: MotionBinding) {
  const host = only(root, binding.target), steps = elements(host, binding.steps), panels = elements(host, binding.panels), bars = elements(host, binding.progress), tracks = elements(host, binding.progressTrack);
  if (!steps.length) return () => {};
  if (panels.length && panels.length !== steps.length || bars.length && bars.length !== steps.length) throw new Error('Each step needs a corresponding captured panel/progress line');
  const originals = [...steps, ...panels, ...bars].map(node => ({ node, selected: node.getAttribute('aria-selected'), hidden: node.hidden, style: node.getAttribute('style') }));
  if (tracks.length && tracks.length !== bars.length) throw new Error('Progress fill needs its matching full track');
  const animations = new Set<Animation>();
  let index = Math.max(0, steps.findIndex(node => node.getAttribute('aria-selected') === 'true')), frame = 0, start = performance.now(), disposed = false, complete = false;
  const choose = (next: number) => {
    index = next; start = performance.now(); complete = false;
    steps.forEach((node, i) => node.setAttribute('aria-selected', String(i === index)));
    panels.forEach((node, i) => { node.hidden = i !== index; if (i === index && !reduced(binding)) trackedAnimation(node,[{opacity:0},{opacity:1}],{duration:Math.min(300,duration(binding,6000)*.4)},animations); });
    bars.forEach((node, i) => { node.style.transformOrigin = 'left'; node.style.transform = `scaleX(${reduced(binding) && i === index ? 1 : 0})`; });
  };
  const handlers = steps.map((node, i) => {
    const click = () => choose(i); node.addEventListener('click', click); return () => node.removeEventListener('click', click);
  });
  choose(index);
  const tick = (now: number) => {
    if (disposed) return;
    if (complete) choose((index + 1) % steps.length);
    const progress = reduced(binding) ? 1 : Math.max(0, Math.min(1, (now - start) / duration(binding, 6000)));
    bars.forEach((node, i) => { if (tracks[i]) node.style.width = `${tracks[i].clientWidth}px`; node.style.transform = `scaleX(${i === index ? progress : 0})`; });
    if (!reduced(binding) && progress === 1 && steps.length > 1) complete = true;
    if (!reduced(binding)) frame = requestAnimationFrame(tick);
  };
  frame = requestAnimationFrame(tick);
  return () => {
    disposed = true; cancelAnimationFrame(frame); handlers.forEach(cleanup => cleanup()); animations.forEach(animation => animation.cancel());
    originals.forEach(({ node, selected, hidden, style }) => { if (selected === null) node.removeAttribute('aria-selected'); else node.setAttribute('aria-selected', selected); node.hidden = hidden; if (style === null) node.removeAttribute('style'); else node.setAttribute('style', style); });
  };
}

function slidingHighlight(root: ParentNode, binding: MotionBinding) {
  const host = only(root, binding.target), indicator = only(host, binding.indicator);
  const originalStyle = indicator.getAttribute('style');
  let animation: Animation | undefined, previous: { x: number; width: number } | undefined, disposed = false;
  const update = () => {
    if (disposed) return;
    const links = elements(host, binding.links), selected = links.filter(node => visible(node) && ['true', 'page'].includes(node.getAttribute('aria-current') ?? node.getAttribute('aria-selected') ?? ''));
    if (selected.length !== 1) return;
    const current = selected[0].getBoundingClientRect(), base = host.getBoundingClientRect();
    const next = { x: current.x - base.x + host.scrollLeft - host.clientLeft, width: current.width };
    if (previous?.x === next.x && previous.width === next.width) return;
    const currentPaint = indicator.getBoundingClientRect();
    animation?.cancel();
    indicator.style.transform = `translateX(${next.x}px)`; indicator.style.width = `${next.width}px`;
    if (previous && !reduced(binding)) animation = indicator.animate([
      { transform: `translateX(${currentPaint.x - base.x + host.scrollLeft - host.clientLeft}px)`, width: `${currentPaint.width}px` },
      { transform: `translateX(${next.x}px)`, width: `${next.width}px` },
    ], { duration: duration(binding, 450), easing: 'cubic-bezier(.22,1,.36,1)' });
    previous = next;
  };
  const mutations = new MutationObserver(update); mutations.observe(host, { subtree: true, childList: true, attributes: true, attributeFilter: ['aria-current', 'aria-selected'] });
  const resize = new ResizeObserver(update); resize.observe(host); elements(host, binding.links).forEach(node => resize.observe(node));
  update(); void document.fonts?.ready.then(update);
  return () => { disposed = true; mutations.disconnect(); resize.disconnect(); animation?.cancel(); if (originalStyle === null) indicator.removeAttribute('style'); else indicator.setAttribute('style', originalStyle); };
}

export function mountMotion(root: ParentNode, bindings: MotionBinding[]) {
  const active = new Map<MotionBinding, {node: HTMLElement; dependencies: HTMLElement[]; cleanup: () => void}>();
  const reveals = bindings.filter(binding => binding.kind === 'section-entrance');
  const seen = new WeakSet<Element>(), revealAnimations = new Set<Animation>();
  const revealObserver = new IntersectionObserver(entries => entries.forEach(entry => {
    if (!entry.isIntersecting) return;
    const node = entry.target as HTMLElement, binding = reveals.find(binding => node.matches(binding.target));
    if (!binding || seen.has(node) || !visible(node)) return;
    seen.add(node); revealObserver.unobserve(node);
    if (!reduced(binding) && !node.contains(document.activeElement)) trackedAnimation(node,[
      { opacity: 0, transform: `translateY(${binding.distancePx ?? 24}px)` }, { opacity: 1, transform: 'translateY(0)' },
    ], { duration: duration(binding, 600), easing: 'ease-out' },revealAnimations);
  }));
  const refresh = () => {
    reveals.forEach(binding => elements(root, binding.target).filter(node => !seen.has(node)).forEach(node => revealObserver.observe(node)));
    for (const binding of bindings) {
      if (binding.kind === 'section-entrance') continue;
      const found = elements(root, binding.target), previous = active.get(binding);
      if (found.length > 1) throw new Error(`Ambiguous motion target: ${binding.target}`);
      const steps = found[0] && binding.kind === 'progress-step-cycle' ? elements(found[0], binding.steps) : [];
      const panels = found[0] && binding.kind === 'progress-step-cycle' ? elements(found[0], binding.panels) : [];
      const bars = found[0] && binding.kind === 'progress-step-cycle' ? elements(found[0], binding.progress) : [];
      const tracks = found[0] && binding.kind === 'progress-step-cycle' ? elements(found[0], binding.progressTrack) : [];
      const marqueeTracks = found[0] && binding.kind === 'marquee' ? elements(found[0], binding.track) : [];
      const marqueeGroups = marqueeTracks.length === 1 ? sourceGroups(marqueeTracks[0], binding.group) : [];
      const dependencies = binding.kind === 'marquee' ? [...marqueeTracks, ...marqueeGroups] : [...steps, ...panels, ...bars, ...tracks];
      if (previous && previous.node === found[0] && previous.dependencies.length === dependencies.length && dependencies.every((node, index) => node === previous.dependencies[index])) continue;
      if (binding.kind === 'marquee' && found[0] && (marqueeTracks.length !== 1 || marqueeGroups.length !== 1)) {
        previous?.cleanup(); active.delete(binding);
        if (marqueeTracks.length > 1 || marqueeGroups.length > 1) throw new Error('Ambiguous original marquee track/group');
        continue;
      }
      // Wait for a coherent streamed/React child update before replacing a
      // functioning cycle. Empty data is coherent and cleans the old one up.
      if (steps.length && (panels.length && panels.length !== steps.length || bars.length && bars.length !== steps.length || tracks.length && tracks.length !== bars.length)) continue;
      const selected = steps.find(node => node.getAttribute('aria-selected') === 'true');
      previous?.cleanup(); active.delete(binding);
      if (found[0]) {
        if (selected && previous?.node === found[0]) steps.forEach(node => node.setAttribute('aria-selected', String(node === selected)));
        const cleanup = ({ 'count-up': countUp, marquee, 'progress-step-cycle': stepCycle, 'sliding-route-highlight': slidingHighlight })[binding.kind](root, binding);
        active.set(binding, {node: found[0], dependencies, cleanup});
      }
    }
  };
  const focus = (event: Event) => revealAnimations.forEach(animation => {
    const node = (animation.effect as KeyframeEffect)?.target;
    if (node instanceof Element && event.target instanceof Node && node.contains(event.target)) animation.cancel();
  });
  const mutations = new MutationObserver(refresh);
  const cleanup = () => {
    mutations.disconnect(); revealObserver.disconnect(); root.removeEventListener('focusin', focus);
    [...active.values()].reverse().forEach(entry => entry.cleanup()); active.clear(); revealAnimations.forEach(animation => animation.cancel());
  };
  try { refresh(); mutations.observe(root, { childList: true, subtree: true }); root.addEventListener('focusin', focus); }
  catch (error) { cleanup(); throw error; }
  return cleanup;
}
