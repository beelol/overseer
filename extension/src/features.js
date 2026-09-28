// Unfinished features in this build (AC-204, the partial merge of Auto routing and Swarm).
// Each is off by default and hidden until its setting is turned on; see
// docs/verification/auto-mode/partial-merge.md for what each still lacks.
const SETTINGS = {
  autoRouting: 'experimental.autoRouting',
  swarm: 'experimental.swarm',
};

/** True only when the owner turned the feature's setting on. A missing workspace API is off. */
function enabled(vscode, feature) {
  const key = SETTINGS[feature];
  if (!key) throw new Error(`unknown feature ${feature}`);
  const config = vscode?.workspace?.getConfiguration?.('overseer');
  return !!config && config.get(key, false) === true;
}

/** The message shown when a hidden command is run anyway (for example from a keybinding). */
function offMessage(feature) {
  const name = feature === 'swarm' ? 'Swarm' : 'Auto routing';
  return `${name} is not finished in this build. Turn on the setting overseer.${SETTINGS[feature]} to try it.`;
}

module.exports = { SETTINGS, enabled, offMessage };
