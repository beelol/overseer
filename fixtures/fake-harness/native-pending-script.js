// Synthetic AC274 transport playback only. No provider, owner profile or native
// capability qualification. Scripts use the frozen installed-shape vectors.
const fs = require('fs');
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
function substitute(value, context) {
  if (typeof value === 'string' && Object.hasOwn(context, value)) return context[value];
  if (Array.isArray(value)) return value.map(v => substitute(v, context));
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, substitute(v, context)]));
  return value;
}
async function play(emit, mark, context) {
  const script = JSON.parse(fs.readFileSync(process.env.FIXTURE_NATIVE_REQUESTS_FILE, 'utf8'));
  if (!Array.isArray(script.steps) || script.steps.length > 64) throw new Error('invalid native fixture script');
  for (const step of script.steps) {
    if (step.gate) {
      const deadline = Date.now() + 30000;
      while (!fs.existsSync(step.gate) && Date.now() < deadline) await wait(10);
      if (!fs.existsSync(step.gate)) throw new Error('native fixture gate timed out');
    }
    if (step.emit) emit(substitute(step.emit, context));
    if (step.mark) mark(step.mark);
  }
}
module.exports = { play };
