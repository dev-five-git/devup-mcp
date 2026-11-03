'use client';

import { useEffect } from 'react';
import { mountMotion, type MotionBinding } from './motion-runtime';

export function MotionController({ bindings }: { bindings: MotionBinding[] }) {
  const configuration = JSON.stringify(bindings);
  useEffect(() => mountMotion(document, JSON.parse(configuration)), [configuration]);
  return null;
}
