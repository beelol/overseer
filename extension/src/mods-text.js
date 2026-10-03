// Words derived from daemon records. A desired plan is never proof of delivery.
const NOISE = 'Planned; not available yet. Less tool noise does not run a transformer.';
const QUALIFICATION = 'Native configuration is unverified. Global text suppression and dynamic local model delivery are unsupported. Child coverage is unknown. Prose quality is untested. Token savings: not measured.';
const escape = value => String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
function safeUrl(value) { try { const u = new URL(String(value)); return ['http:', 'https:'].includes(u.protocol) && !u.username && !u.password ? u.href : null; } catch { return null; } }
function scope(s = {}) { return ({ all_agents: 'All agents (excludes Overseer and watchers)', watchers: 'Watchers', overseer: 'Overseer' })[s.kind] || (s.kind === 'repository' ? `Repository: ${s.repo_key}` : s.kind === 'agent' ? `One agent: ${s.run_id}` : 'Unknown scope'); }
function library(data) { return (data?.installed || []).map(v => ({ ...v, name: v.manifest?.name || v.id, summary: v.manifest?.summary || '', pin: `Version ${v.version} · ${v.fingerprint}`, homepage: safeUrl(v.manifest?.homepage), state: (data.bindings || []).some(b => b.fingerprint === v.fingerprint && b.enabled) ? 'Installed; enabled bindings exist' : 'Installed; no enabled bindings' })); }
function applied(data) {
  if (!data) return { state: 'Choose an agent to inspect its turns', desired: 'Not inspected', last: 'No recorded delivery', decisions: [], coverage: 'Child coverage: unknown' };
  const desired = data.desired || {}, last = data.last_turn;
  const names = (desired.versions || []).map(v => v.manifest?.name || v.id || v.mod_id).filter(Boolean);
  return { state: data.pending ? `Pending next ${(desired.decisions || []).some(d => d.activation === 'next_thread' && d.status === 'selected') ? 'thread' : (desired.decisions || []).some(d => d.activation === 'next_thread') ? 'thread' : 'turn'}` : !last ? 'No recorded turn delivery' : 'Desired and last recorded state match',
    desired: names.length ? names.join(', ') : 'No mod text selected',
    last: !last ? 'No recorded delivery' : last.outcome === 'transport_accepted' ? (last.delivery === 'message_text' ? `Message text accepted by transport · ${last.added_bytes || 0} added bytes` : `Transport accepted · ${last.delivery || 'unknown delivery'}`) : ({ prepared: 'Prepared; transport acceptance not recorded', failed_before_effect: 'Launch failed before effects', uncertain_after_effect: 'Launch outcome uncertain after effects' }[last.outcome] || 'Delivery outcome unknown'),
    decisions: (desired.decisions || []).map(d => `${d.mod_id}: ${d.status} · ${d.reason} · ${d.delivery} · ${d.activation} · children ${d.children || 'unknown'}`), coverage: 'Child coverage: unknown',
    notice: data.notice || '', snapshot: last };
}
module.exports = { NOISE, QUALIFICATION, escape, safeUrl, scope, library, applied };
