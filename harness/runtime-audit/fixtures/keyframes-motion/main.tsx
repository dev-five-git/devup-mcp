import { hydrateRoot } from 'react-dom/client';
import { App } from './App';

declare global { interface Window { stopMotion: () => void; prerendered?: string } }

const params = new URLSearchParams(location.search);
if (params.has('prerender')) {
  // The test reads this once and writes it into index.html, so the page the
  // checks load is server markup produced by this same bundle.
  const { renderToString } = await import('react-dom/server');
  window.prerendered = renderToString(<App initialPath="/" />);
} else {
  // `?timed` forces the marker path that browsers without view timelines take.
  if (params.has('timed')) CSS.supports = () => false;
  const instance = hydrateRoot(document.getElementById('root')!, <App initialPath="/" />);
  window.stopMotion = () => instance.unmount();
}
