// The keyframes entrance exactly as devup-mcp ships it (Reveal.tsx,
// MotionController.tsx and motion-runtime.ts are copied in beside this file
// by the test), compiled by the Devup UI plugin and rendered on the server.
import { Box, css } from '@devup-ui/react';
import { useEffect, useState } from 'react';
import { flushSync } from 'react-dom';
import { MotionController } from './MotionController';
import { Reveal, RevealGroup, RevealItem, RevealLine, RevealStep } from './Reveal';

const cards = ['Regenerate', 'Restore', 'Protect', 'Recover'];
const steps = ['Discovery', 'Preclinical', 'Clinical'];
const bindings = [{ kind: 'section-entrance' as const, target: '[data-reveal]' }];

function Home() {
  return (
    <Box as="main">
      <Box as="header" h="1400px" p="24px">
        <h1>First viewport: no entrance</h1>
        <a id="other" href="/other">Another route</a>
      </Box>
      {/* The painted band stays still; the wrapper inside it moves. */}
      <Box as="section" id="band" bg="#EEF2FF" py="40px">
        <Reveal id="reveal">
          <Box h="200px" p="16px">
            <h2>Readable server section</h2>
            <a href="#detail">Detail</a>
            <img src="/pixel.png" width="40" height="40" alt="" />
          </Box>
        </Reveal>
      </Box>
      <Box h="900px" />
      <RevealGroup id="cards" className={css({ display: 'flex', gap: '8px', flexWrap: 'wrap' })}>
        {cards.map((card, index) => (
          <RevealItem key={card} index={index} className="card">
            <Box w="150px" h="120px" bg="#F1F5F9" p="12px">{card}</Box>
          </RevealItem>
        ))}
      </RevealGroup>
      <Box h="900px" />
      <RevealGroup id="timeline" className={css({ position: 'relative', h: '400px' })}>
        <RevealLine className={`line ${css({ position: 'absolute', left: '0', top: '0', w: '2px', h: '100%', bg: '#2563EB' })}`} />
        {steps.map((step, index) => (
          <RevealStep key={step} index={index} of={steps.length} className="step" style={{ position: 'absolute', left: 20, top: index * 133, height: 40 }}>
            {step}
          </RevealStep>
        ))}
      </RevealGroup>
      <Box h="1400px" />
    </Box>
  );
}

// The smallest client router that behaves like a framework's: same document,
// history entries, scroll restored on the way back.
export function App({ initialPath = '/' }: { initialPath?: string }) {
  const [path, setPath] = useState(initialPath);
  useEffect(() => {
    const click = (event: MouseEvent) => {
      const link = (event.target as Element).closest('a[href^="/"]');
      if (!link) return;
      event.preventDefault();
      history.replaceState({ y: scrollY }, '');
      history.pushState({ y: 0 }, '', link.getAttribute('href'));
      flushSync(() => setPath(location.pathname));
      scrollTo(0, 0);
    };
    const pop = (event: PopStateEvent) => {
      flushSync(() => setPath(location.pathname));
      scrollTo(0, event.state?.y ?? 0);
    };
    document.addEventListener('click', click);
    addEventListener('popstate', pop);
    return () => { document.removeEventListener('click', click); removeEventListener('popstate', pop); };
  }, []);
  return (
    <>
      {path === '/other' ? <Box as="main" h="600px" p="24px"><h1>Another route</h1><a href="/">Home</a></Box> : <Home />}
      <MotionController bindings={bindings} />
    </>
  );
}
