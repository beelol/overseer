// AC-204: unfinished Auto routing and Swarm surfaces are hidden unless their settings are on.
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const manifest = require('../../extension/package.json');
const features = require('../../extension/src/features');

const props = manifest.contributes.configuration.properties;
for (const [feature, key] of Object.entries(features.SETTINGS)) {
  const setting = props[`overseer.${key}`];
  assert(setting, `setting overseer.${key} is declared`);
  assert.equal(setting.type, 'boolean');
  assert.equal(setting.default, false, `${feature} is off by default`);
  assert.match(setting.markdownDescription, /Unfinished/);
}

// Every command of an unfinished feature is disabled, hidden from the palette and
// from every menu unless its setting is on.
const gated = {
  swarm: ['startSwarm', 'filterSwarmJobs', 'pauseSwarm', 'resumeSwarm', 'stopSwarm', 'turnSwarmOff', 'extendSwarmDeadline'],
};
// Auto Usage stays visible: the local learning history it inspects, exports and
// clears is recorded for every run, so the owner can always see and clear it.
const usage = manifest.contributes.commands.find(c => c.command === 'overseer.autoUsage');
assert(usage && !usage.enablement, 'Auto Usage is always available');
assert(!(manifest.contributes.menus.commandPalette || []).some(m => m.command === 'overseer.autoUsage'));
const menus = Object.values(manifest.contributes.menus).flat();
for (const [feature, commands] of Object.entries(gated)) {
  const clause = `config.overseer.${features.SETTINGS[feature]}`;
  for (const name of commands) {
    const id = `overseer.${name}`;
    const command = manifest.contributes.commands.find(c => c.command === id);
    assert(command, `${id} is declared`);
    assert(command.enablement && command.enablement.includes(clause), `${id} is disabled while ${clause} is off`);
    const palette = manifest.contributes.menus.commandPalette.find(m => m.command === id);
    assert(palette && palette.when.includes(clause), `${id} is hidden from the command palette while off`);
    for (const entry of menus.filter(m => m.command === id)) {
      assert(entry.when && entry.when.includes(clause), `${id} menu entry is hidden while off: ${entry.when}`);
    }
  }
}

// A command run anyway (a keybinding) is guarded in code, and the views and the
// composer read the same settings.
const extension = fs.readFileSync(path.join(__dirname, '../../extension/src/extension.js'), 'utf8');
for (const [feature, commands] of Object.entries(gated)) {
  for (const name of commands) {
    assert(new RegExp(`registerCommand\\('overseer\\.${name}', guard\\(whenOn\\('${feature}'`).test(extension),
      `overseer.${name} handler checks ${feature}`);
  }
}
const composer = fs.readFileSync(path.join(__dirname, '../../extension/media/composer.js'), 'utf8');
assert(/!data\.autoRouting \? \[\]/.test(composer), 'the composer offers Auto routing only when on');
assert(/routing: d\.autoRouting \?/.test(composer), 'a remembered Auto choice is dropped while off');
const form = fs.readFileSync(path.join(__dirname, '../../extension/media/new-task.js'), 'utf8');
assert(/const autoTile = data\.autoRouting \?/.test(form), 'the New Task form offers Auto routing only when on');

// The settings reader: off unless explicitly true, and off without a workspace API.
const stub = value => ({ workspace: { getConfiguration: () => ({ get: () => value }) } });
assert.equal(features.enabled(stub(true), 'swarm'), true);
assert.equal(features.enabled(stub(false), 'swarm'), false);
assert.equal(features.enabled(stub('yes'), 'autoRouting'), false);
assert.equal(features.enabled({}, 'autoRouting'), false);
assert.throws(() => features.enabled(stub(true), 'other'));
assert.match(features.offMessage('swarm'), /overseer\.experimental\.swarm/);
console.log('unfinished features are hidden by default');
