import { statistics } from './statistics';
self.onmessage = event => {
  const { id, original, modified } = event.data;
  try { self.postMessage({ id, ...statistics(original, modified) }); }
  catch { self.postMessage({ id, reason: 'Diff classification failed. Counts are unavailable.' }); }
};
