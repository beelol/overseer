// AC-246: one name for each thing. What Overseer starts is an "agent" for the owner, never a "task"
// or a "run": a check over package.json's titles, descriptions and welcome text, and over the
// words the extension shows (string literals in extension/src and extension/media). "The task"
// as the work an agent is given ("Describe the task") is plain English and allowed; naming the
// thing a task or a run is not. "Dashboard" (one agent full screen, not an overview of all agents)
// is called Focus Mode. Run: node test/unit/one-name.js
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '../..');

// The thing called a task or a run.
const BANNED = [
  /\bNew Task\b/i, /\bStart Task\b/i, /\bStart task\b/, /\bagent tasks?\b/i, /\bTask prompt\b/i, /\bNew task\b/, /\btask history\b/i, /\bThis task\b/i, /\*\*Task\*\*/,
  /\bDashboard\b/, /\bthe dashboard\b/,
  /\bCopy run ID\b/i, /\bNo run\b/, /\b(?:a|the|this|each|every|your|existing|finished|failed) run\b/i, /\bruns you\b/i, /\bSwarm runs\b/i, /\bthis run's\b/i, /\bthe run's\b/i, /\brun panel\b/i,
];
const hits = [];
const look = (where, text) => { for (const re of BANNED) { const m = re.exec(text); if (m) hits.push(`${where}: "${m[0]}" in ${JSON.stringify(text.slice(0, 140))}`); } };

// package.json: every title, description and welcome text.
const pkg = JSON.parse(fs.readFileSync(path.join(root, 'extension/package.json'), 'utf8'));
const c = pkg.contributes;
for (const x of c.commands) look(`command ${x.command}`, x.title);
for (const [k, v] of Object.entries(c.configuration.properties)) look(`setting ${k}`, [v.description, v.markdownDescription, ...(v.enumDescriptions || [])].filter(Boolean).join(' '));
for (const w of c.viewsWelcome || []) look(`welcome ${w.view}`, w.contents);
for (const [k, list] of Object.entries(c.views || {})) for (const v of list) look(`view ${v.id}`, v.name);
look('package description', pkg.description || '');

// The words the extension shows: string literals with a space in them (sentences, labels).
const files = [...fs.readdirSync(path.join(root, 'extension/src')).map(f => `extension/src/${f}`), ...fs.readdirSync(path.join(root, 'extension/media')).filter(f => f.endsWith('.js')).map(f => `extension/media/${f}`)].filter(f => f.endsWith('.js'));
for (const f of files) {
  const src = fs.readFileSync(path.join(root, f), 'utf8').split('\n').filter(l => !/^\s*(\/\/|\*|\/\*)/.test(l)).join('\n');
  for (const m of src.matchAll(/'([^'\n]*\s[^'\n]*)'|`([^`\n]*\s[^`\n]*)`|"([^"\n]*\s[^"\n]*)"/g)) {
    const s = m[1] || m[2] || m[3];
    if (/\brequire\(|\bsay\(|\blog\(/.test(s)) continue;
    look(f, s);
  }
}
for (const h of hits) console.log('FAIL', h);
console.log(hits.length ? `${hits.length} place(s) call an agent a task or a run` : 'an agent is an agent everywhere the owner reads');
process.exit(hits.length ? 1 : 0);
