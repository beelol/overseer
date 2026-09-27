// AC-159: when the system git refuses to run (for example an unaccepted Xcode license), use the
// Command Line Tools' git for this process and everything it starts, and say so in one line.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');

const CLT = '/Library/Developer/CommandLineTools';
const works = env => cp.spawnSync('git', ['--version'], { env, encoding: 'utf8' }).status === 0;

function ensureGit(label = 'overseer') {
  if (works(process.env)) return false;
  if (fs.existsSync(path.join(CLT, 'usr/bin/git')) && works({ ...process.env, DEVELOPER_DIR: CLT })) {
    process.env.DEVELOPER_DIR = CLT;
    console.log(`note (${label}): the system git would not run; using the Command Line Tools git`);
    return true;
  }
  return false;
}

module.exports = { ensureGit };
