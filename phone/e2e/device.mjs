// The two simulators, behind one set of functions: what the scenario run and the measurements
// need of a device. Overseer's own devices only: the iOS simulator named below and the Android
// virtual device Overseer_API_35. No other project's device is listed, started or changed.

import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import process from 'node:process';

export const BUNDLE = 'com.beelol.overseer.phone';
export const IOS_SIMULATOR = process.env.OVERSEER_IOS_SIMULATOR || 'iPhone 17 Pro';
export const ANDROID_AVD = 'Overseer_API_35';
const ANDROID_HOME = process.env.ANDROID_HOME || '/opt/homebrew/share/android-commandlinetools';
const ADB = path.join(ANDROID_HOME, 'platform-tools', 'adb');
const KV = 'ExpoSQLiteStorage';

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function out(command, args, options = {}) {
  return execFileSync(command, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], ...options }).trim();
}

function quiet(command, args) {
  try {
    return out(command, args);
  } catch {
    return '';
  }
}

/** The emulator's PIN while the safety settings are tried: set, typed and cleared by the run. */
export const PIN = '1357';

const sql = (text) => `'${String(text).replaceAll("'", "''")}'`;

function iosDevice() {
  const listed = JSON.parse(out('xcrun', ['simctl', 'list', 'devices', 'available', '--json']));
  const found = Object.entries(listed.devices)
    .filter(([runtime]) => runtime.includes('iOS'))
    .flatMap(([, devices]) => devices)
    .filter((device) => device.name === IOS_SIMULATOR && device.isAvailable)
    .sort((a, b) => Number(b.state === 'Booted') - Number(a.state === 'Booted'))[0];
  if (!found) throw new Error(`no available iOS simulator is named "${IOS_SIMULATOR}"`);
  if (found.state !== 'Booted') throw new Error(`the simulator "${IOS_SIMULATOR}" is not booted. Start it: npm run ios:release`);
  return found.udid;
}

function ios() {
  const udid = iosDevice();
  const simctl = (...args) => out('xcrun', ['simctl', ...args]);
  const database = () => path.join(simctl('get_app_container', udid, BUNDLE, 'data'), 'Documents', 'SQLite', KV);
  return {
    platform: 'ios',
    id: udid,
    /** The Mac, as this device reaches it. */
    host: '127.0.0.1',
    installed: () => quiet('xcrun', ['simctl', 'get_app_container', udid, BUNDLE]) !== '',
    install: (app) => void simctl('install', udid, app),
    uninstall: () => void quiet('xcrun', ['simctl', 'uninstall', udid, BUNDLE]),
    launch: () => void simctl('launch', udid, BUNDLE),
    /** How far the device's clock is ahead of the Mac's, in milliseconds: the simulator uses the Mac's own. */
    clockOffset: () => 0,
    // The device's own unlock, for the safety settings: Face ID enrolled on Overseer's simulator
    // and matched through the simulator's biometric notifications (what Simulator's Features menu does).
    unlockSetUp: () => {
      simctl('spawn', udid, 'notifyutil', '-s', 'com.apple.BiometricKit.enrollmentChanged', '1');
      simctl('spawn', udid, 'notifyutil', '-p', 'com.apple.BiometricKit.enrollmentChanged');
    },
    unlockMatch: () => void simctl('spawn', udid, 'notifyutil', '-p', 'com.apple.BiometricKit_Sim.pearl.match'),
    /** A photo in the simulator's library, for the photo picker. */
    addPhoto: (file) => void simctl('addmedia', udid, file),
    /** Everything the app keeps on disk, copied to `dest`: more than a backup of the app carries. */
    appFiles: (dest) => void execFileSync('cp', ['-R', `${simctl('get_app_container', udid, BUNDLE, 'data')}/.`, dest]),
    /** The app's items in this simulator's keychain: their access group and accessibility class. */
    keychainItems: () => {
      const db = path.join(os.homedir(), 'Library', 'Developer', 'CoreSimulator', 'Devices', udid, 'data', 'Library', 'Keychains', 'keychain-2-debug.db');
      const rows = quiet('sqlite3', ['-separator', '\t', db, "select agrp, pdmn from genp where agrp like '%overseer%'"]);
      return rows.split('\n').filter(Boolean).map((row) => {
        const [group, accessible] = row.split('\t');
        return { group, accessible };
      });
    },
    unlockTearDown: () => {
      simctl('spawn', udid, 'notifyutil', '-s', 'com.apple.BiometricKit.enrollmentChanged', '0');
      simctl('spawn', udid, 'notifyutil', '-p', 'com.apple.BiometricKit.enrollmentChanged');
    },
    /** Shuts the simulator down and boots it again; returns once it has booted. */
    reboot: () => {
      quiet('xcrun', ['simctl', 'shutdown', udid]);
      simctl('boot', udid);
      out('xcrun', ['simctl', 'bootstatus', udid, '-b']);
    },
    stop: () => void quiet('xcrun', ['simctl', 'terminate', udid, BUNDLE]),
    appearance: (mode) => void simctl('ui', udid, 'appearance', mode),
    /** The system's text size: `small`, `standard` or `large` (the largest standard size). */
    textSize: (size) => void simctl('ui', udid, 'content_size', { small: 'extra-small', standard: 'large', large: 'extra-extra-extra-large' }[size]),
    read: (key) => quiet('sqlite3', [database(), `select value from storage where key=${sql(key)}`]),
    keys: () => (quiet('sqlite3', [database(), 'select key from storage order by key']) || '').split('\n').filter(Boolean),
    write: (key, value) => void out('sqlite3', [database(), `create table if not exists storage (key text primary key not null, value text); insert or replace into storage(key, value) values(${sql(key)}, ${sql(JSON.stringify(value))})`]),
    remove: (key) => void quiet('sqlite3', [database(), `delete from storage where key=${sql(key)}`]),
    screenshot: (file) => void simctl('io', udid, 'screenshot', file),
    push: (payloadFile) => void simctl('push', udid, BUNDLE, payloadFile),
    /** Records the screen until the returned function is called; resolves with the file. */
    record(file) {
      fs.rmSync(file, { force: true });
      const child = spawn('xcrun', ['simctl', 'io', udid, 'recordVideo', '--codec', 'h264', '--force', file], { stdio: 'ignore' });
      return async () => {
        child.kill('SIGINT');
        await new Promise((resolve) => child.on('exit', resolve));
        return file;
      };
    },
  };
}

function android() {
  const adb = (...args) => out(ADB, args);
  const devices = adb('devices').split('\n').slice(1).map((line) => line.split('\t')).filter(([, state]) => state === 'device').map(([serial]) => serial);
  const serial = devices.find((s) => quiet(ADB, ['-s', s, 'emu', 'avd', 'name']).split('\n')[0].trim() === ANDROID_AVD);
  if (!serial) throw new Error(`the Android virtual device ${ANDROID_AVD} is not running. Start it: npm run android:release`);
  const shell = (command) => out(ADB, ['-s', serial, 'shell', command]);
  // The storage of a release build is read as root, which the emulator's image allows.
  quiet(ADB, ['-s', serial, 'root']);
  const database = `/data/data/${BUNDLE}/files/SQLite/${KV}`;
  const owner = () => shell(`stat -c %U:%G /data/data/${BUNDLE}`);
  return {
    platform: 'android',
    id: serial,
    host: '10.0.2.2',
    installed: () => shell(`pm list packages ${BUNDLE}`).includes(BUNDLE),
    install: (apk) => void adb('-s', serial, 'install', '-r', apk),
    uninstall: () => void quiet(ADB, ['-s', serial, 'uninstall', BUNDLE]),
    /** How far the emulator's clock is ahead of the Mac's, in milliseconds: the median of three readings. */
    clockOffset: () => {
      const readings = [0, 1, 2].map(() => {
        const before = Date.now();
        const device = Number(shell('date +%s%3N'));
        return device - (before + Date.now()) / 2;
      });
      return Math.round(readings.sort((a, b) => a - b)[1]);
    },
    // The device's own unlock, for the safety settings: a PIN on Overseer's own virtual device,
    // typed by the flow into the system's prompt, and cleared afterwards.
    unlockSetUp: () => void shell(`locksettings set-pin ${PIN}`),
    unlockMatch: () => undefined,
    /** A photo in the emulator's library, for the photo picker. */
    addPhoto: (file) => {
      adb('-s', serial, 'push', file, '/sdcard/Pictures/overseer-photo.png');
      shell('am broadcast -a android.intent.action.MEDIA_SCANNER_SCAN_FILE -d file:///sdcard/Pictures/overseer-photo.png');
    },
    /** Everything the app keeps on disk, copied to `dest` as root: more than a backup of the app carries. */
    appFiles: (dest) => {
      const tar = execFileSync(ADB, ['-s', serial, 'exec-out', `tar -C /data/data/${BUNDLE} -cf - .`], { maxBuffer: 512 * 1024 * 1024 });
      fs.writeFileSync(path.join(dest, 'app.tar'), tar);
      execFileSync('tar', ['-xf', path.join(dest, 'app.tar'), '-C', dest]);
      fs.rmSync(path.join(dest, 'app.tar'));
    },
    /** Android keeps no keychain of this kind: the Keystore's keys never leave it. */
    keychainItems: () => [],
    unlockTearDown: () => void quiet(ADB, ['-s', serial, 'shell', `locksettings clear --old ${PIN}`]),
    // `monkey` reports failure (exit 251) on this emulator image and starts nothing; the activity is started by name.
    launch: () => void shell(`am start -n ${BUNDLE}/.MainActivity >/dev/null 2>&1`),
    /** Reboots the emulator and waits until Android says it has booted; storage is read as root again afterwards. */
    reboot: () => {
      adb('-s', serial, 'reboot');
      adb('-s', serial, 'wait-for-device');
      for (let i = 0; i < 120; i += 1) {
        if (quiet(ADB, ['-s', serial, 'shell', 'getprop', 'sys.boot_completed']).trim() === '1') break;
        execFileSync('sleep', ['1']);
      }
      quiet(ADB, ['-s', serial, 'root']);
      adb('-s', serial, 'wait-for-device');
    },
    stop: () => void shell(`am force-stop ${BUNDLE}`),
    appearance: (mode) => void shell(`cmd uimode night ${mode === 'dark' ? 'yes' : 'no'}`),
    textSize: (size) => void shell(`settings put system font_scale ${{ small: '0.85', standard: '1.0', large: '1.3' }[size]}`),
    read: (key) => quiet(ADB, ['-s', serial, 'shell', `sqlite3 ${database} "select value from storage where key=${sql(key)}"`]),
    keys: () => (quiet(ADB, ['-s', serial, 'shell', `sqlite3 ${database} "select key from storage order by key"`]) || '').split('\n').map((line) => line.trim()).filter(Boolean),
    write(key, value) {
      const text = JSON.stringify(value).replaceAll('"', '\\"');
      shell(`mkdir -p /data/data/${BUNDLE}/files/SQLite && sqlite3 ${database} "create table if not exists storage (key text primary key not null, value text); insert or replace into storage(key, value) values(${sql(key)}, '${text}')" && chown -R ${owner()} /data/data/${BUNDLE}/files`);
    },
    remove: (key) => void quiet(ADB, ['-s', serial, 'shell', `sqlite3 ${database} "delete from storage where key=${sql(key)}"`]),
    screenshot(file) {
      fs.writeFileSync(file, execFileSync(ADB, ['-s', serial, 'exec-out', 'screencap', '-p'], { maxBuffer: 64 * 1024 * 1024 }));
    },
    push: () => {
      throw new Error('Android shows its notifications itself in this gate; nothing is pushed to it');
    },
    record(file) {
      const remote = '/sdcard/overseer-record.mp4';
      quiet(ADB, ['-s', serial, 'shell', `rm -f ${remote}`]);
      const child = spawn(ADB, ['-s', serial, 'shell', `screenrecord --bit-rate 8000000 --time-limit 30 ${remote}`], { stdio: 'ignore' });
      return async () => {
        quiet(ADB, ['-s', serial, 'shell', 'pkill -INT screenrecord']);
        await new Promise((resolve) => child.on('exit', resolve));
        await sleep(500);
        adb('-s', serial, 'pull', remote, file);
        return file;
      };
    },
  };
}

/** The device of a platform, ready to be driven. */
export function device(platform) {
  if (platform === 'ios') return ios();
  if (platform === 'android') return android();
  throw new Error(`unknown platform: ${platform} (known: ios, android)`);
}

/** What the app stored under `key`, read as the value it stored, or null. */
export function stored(dev, key) {
  const raw = dev.read(key);
  if (!raw) return null;
  try {
    const value = JSON.parse(raw);
    return typeof value === 'string' && /^[[{]/.test(value) ? JSON.parse(value) : value;
  } catch {
    return null;
  }
}

/** Maestro, with the Java it needs and no analytics. */
export function maestro(args, options = {}) {
  const java = process.env.JAVA_HOME || path.join(os.homedir(), '.sdkman', 'candidates', 'java', '17.0.19-tem');
  return spawn('maestro', args, {
    stdio: ['ignore', 'pipe', 'pipe'],
    ...options,
    env: { ...process.env, JAVA_HOME: java, ANDROID_HOME, PATH: `${path.join(java, 'bin')}:${path.join(ANDROID_HOME, 'platform-tools')}:${process.env.PATH}`, MAESTRO_CLI_NO_ANALYTICS: '1', MAESTRO_CLI_ANALYSIS_NOTIFICATION_DISABLED: 'true', ...options.env },
  });
}
