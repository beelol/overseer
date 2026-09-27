// Packaged-UI scenario for AC-142 (no paid tokens): one Overseer mark everywhere. The VSIX's
// Marketplace icon is the owner's app icon; the installed notification helper carries it; the
// activity bar shows the single-colour mark (Overseer Dark, Overseer Light, High Contrast and the bold
// Overseer theme), the status bar shows it as a product icon, the Overseer view's tab and the
// composer's "What's next?" heading show the full-colour mark; and a search
// of the repository finds no other Overseer logo (the old eye).
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

const THEMES = ['Overseer Dark', 'Overseer Light', 'Default High Contrast', 'Overseer'];
const slug = t => t.replace(/^Default /, '').toLowerCase().replace(/\s+/g, '-');
const OLD_EYE = ['M2 12s3.6-7 10-7', 'M232 512s104-196']; // the retired eye glyph (activity bar, composer) and the old helper icon
const pad = (r, x = 8, y = 8) => ({ x: Math.max(0, r.x - x), y: Math.max(0, r.y - y), width: r.width + 2 * x, height: r.height + 2 * y });

function repoSearch() {
  const tracked = cp.execFileSync('git', ['ls-files'], { cwd: repoRoot, encoding: 'utf8' }).split('\n').filter(f => f && !f.startsWith('docs/verification/') && !/\.(png|icns|woff|ttf|vsix)$/.test(f) && !f.includes('/vendor/'));
  const hits = [];
  for (const f of tracked) {
    let text; try { text = fs.readFileSync(path.join(repoRoot, f), 'utf8'); } catch { continue; }
    text.split('\n').forEach((line, i) => {
      if (OLD_EYE.some(p => line.includes(p))) hits.push(`${f}:${i + 1}: old eye glyph`);
      // The eye as Overseer's logo: a line that names Overseer and draws the eye codicon.
      if (/^(extension\/(src|media|notifier)\/|README\.md)/.test(f) && /\$\(eye\)|codicon-eye\b|icon\('eye'|\(eye\) icon/.test(line) && /Overseer/.test(line)) hits.push(`${f}:${i + 1}: eye beside Overseer`);
    });
  }
  const gone = ['extension/notifier/AppIcon.svg'].filter(f => fs.existsSync(path.join(repoRoot, f)));
  return { files: tracked.length, hits, oldFilesPresent: gone };
}

(async () => {
  const s = new Session('brand');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    // Brand files in each size a surface asks for.
    const brand = path.join(repoRoot, 'docs/design/brand');
    const size = f => { const o = cp.execFileSync('sips', ['-g', 'pixelWidth', '-g', 'pixelHeight', f], { encoding: 'utf8' }); return [...o.matchAll(/pixel(?:Width|Height): (\d+)/g)].map(m => +m[1]).join('x'); };
    const wanted = { 'overseer-app-icon': [128, 256, 512, 1024], 'overseer-logo': [64, 128, 256], 'overseer-icon-flat': [32, 64, 128] };
    const sizes = {};
    for (const [name, list] of Object.entries(wanted)) for (const n of list) { const f = path.join(brand, 'exports', `${name}-${n}.png`); sizes[`${name}-${n}`] = fs.existsSync(f) ? size(f) : 'missing'; }
    sizes['overseer-app-icon-macos-1024'] = size(path.join(brand, 'exports/overseer-app-icon-macos-1024.png'));
    const svg = fs.readFileSync(path.join(brand, 'overseer-mark.svg'), 'utf8');
    check('the brand files exist in each size (app icon 128-1024 and macOS 1024, logo 64-256, flat 32-128) with the single-colour SVG',
      Object.entries(sizes).every(([k, v]) => v === `${k.match(/(\d+)$/)[1]}x${k.match(/(\d+)$/)[1]}`) && /fill="currentColor"/.test(svg) && !/stroke=/.test(svg), sizes);
    check('extension/media/overseer.svg (the activity bar icon) is the single-colour mark', fs.readFileSync(path.join(repoRoot, 'extension/media/overseer.svg'), 'utf8') === svg);

    // The Marketplace icon in the VSIX.
    const vsix = latestVsix();
    const manifest = cp.execFileSync('unzip', ['-p', vsix, 'extension/package.json'], { encoding: 'utf8' });
    const icon = JSON.parse(manifest).icon;
    const iconFile = path.join(s.evidence, 'vsix-marketplace-icon.png');
    fs.writeFileSync(iconFile, cp.execFileSync('unzip', ['-p', vsix, `extension/${icon}`]));
    const vsixManifest = cp.execFileSync('unzip', ['-p', vsix, 'extension.vsixmanifest'], { encoding: 'utf8' });
    check('the VSIX declares the owner\'s app icon at 256 px as its Marketplace icon', icon === 'media/overseer-app-icon.png' && size(iconFile) === '256x256' && vsixManifest.includes(`<Icon>extension/${icon}</Icon>`),
      { icon, size: size(iconFile), evidence: path.relative(repoRoot, iconFile) });

    // Repository search: no other Overseer logo.
    const search = repoSearch();
    check('a search of the repository finds no other Overseer logo (the old eye glyph)', search.hits.length === 0 && search.oldFilesPresent.length === 0, search);

    const repo = makeRepo(path.join(s.root, 'brand-demo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': THEMES[0] });
    s.install(latestVsix());

    // The notification helper's icon, as installed.
    const ext = path.join(s.extensions, fs.readdirSync(s.extensions).find(d => d.startsWith('beelol.overseer')));
    const icns = path.join(ext, 'bin/Overseer Notifier.app/Contents/Resources/AppIcon.icns');
    if (process.platform === 'darwin') {
      const helperPng = path.join(s.evidence, 'notifier-app-icon.png');
      cp.execFileSync('sips', ['-s', 'format', 'png', icns, '--out', helperPng], { stdio: 'ignore' });
      const set = path.join(s.root, 'AppIcon.iconset');
      cp.execFileSync('iconutil', ['-c', 'iconset', icns, '-o', set]);
      const reps = fs.readdirSync(set).sort();
      // The owner's icon is a dark violet tile: the centre of the tile's top edge is violet, not the old blue-grey.
      const px = cp.execFileSync('magick', [path.join(set, 'icon_512x512@2x.png'), '-format', '%[fx:int(255*p{512,140}.r)],%[fx:int(255*p{512,140}.g)],%[fx:int(255*p{512,140}.b)]', 'info:'], { encoding: 'utf8' }).trim().split(',').map(Number);
      check('the installed notification helper\'s icon is the owner\'s app icon (every macOS size, violet tile)', reps.length >= 10 && px[2] > px[1] && px[0] > px[1],
        { reps, sample: px, evidence: path.relative(repoRoot, helperPng) });
    }

    s.launch(repo, {});
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const setTheme = async theme => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = theme; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(2000); };
    const frameRect = async (frame, selector) => {
      const inner = await frame.eval(`(() => { const e = document.querySelector(${JSON.stringify(selector)}); if (!e) return null; const r = e.getBoundingClientRect(); return { x: r.left, y: r.top, width: r.width, height: r.height, w: innerWidth, h: innerHeight }; })()`);
      if (!inner) return null;
      const frames = await cdp.evalWorkbench(`[...document.querySelectorAll('iframe.webview')].map(f => { const r = f.getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height, src: f.src }; }).filter(r => r.w > 0 && r.h > 0)`);
      const origin = frame.context.origin || '';
      const m = frames.find(f => origin && f.src.startsWith(origin)) || frames.find(f => Math.abs(f.w - inner.w) < 3 && Math.abs(f.h - inner.h) < 3) || frames[0];
      return { x: m.x + inner.x, y: m.y + inner.y, width: inner.width, height: inner.height };
    };
    const MARK_PROBE = sel => `(async () => { const m = document.querySelector(${JSON.stringify(sel)}); if (!m || !m.offsetParent) return null; const cs = getComputedStyle(m); const url = (cs.backgroundImage.match(/url\\("?(.*?)"?\\)/) || [])[1];
      const img = url ? await new Promise(res => { const i = new Image(); i.onload = () => res(i.naturalWidth + 'x' + i.naturalHeight); i.onerror = () => res('error'); i.src = url; }) : null;
      return { cls: m.className, label: m.getAttribute('aria-label'), image: url && url.split('/').pop(), loaded: img, size: Math.round(m.getBoundingClientRect().width), svg: !!m.querySelector('svg'), eye: !!document.querySelector('.composer-hero > .hero-mark .codicon-eye, .hero-title .codicon-eye') }; })()`;

    await cdp.command('Overseer: Open Overseer View'); await delay(1500);
    const seen = {};
    for (const theme of THEMES) {
      await setTheme(theme);
      const t = slug(theme);
      // Activity bar: the single-colour mark, tinted by the theme.
      const bar = await cdp.evalWorkbench(`(() => { const a = [...document.querySelectorAll('.activitybar .action-item')].find(i => /^Overseer/.test(i.querySelector('.action-label')?.getAttribute('aria-label') || '')); if (!a) return null;
        const l = a.querySelector('.action-label'); const cs = getComputedStyle(l); const r = a.getBoundingClientRect(); const bar = document.querySelector('.activitybar').getBoundingClientRect();
        return { mask: (cs.webkitMaskImage || cs.maskImage || '').split('/').pop().replace(/[")]/g, ''), tint: cs.backgroundColor, rect: { x: bar.left, y: r.top - 48, width: bar.width, height: r.height + 96 } }; })()`);
      if (bar) await s.screenshot(`activity-bar-${t}`, pad(bar.rect, 0, 0));
      // Status bar: the mark as a product icon from the contributed font.
      const status = await cdp.evalWorkbench(`(() => { const it = [...document.querySelectorAll('.statusbar-item')].find(e => /Overseer \\d+ active/.test(e.textContent)); if (!it) return null;
        const g = it.querySelector('.codicon-overseer-mark'); const b = g && getComputedStyle(g, '::before'); const r = it.getBoundingClientRect();
        return { glyph: !!g, content: b && b.content, font: b && b.fontFamily, eye: !!it.querySelector('.codicon-eye'), rect: { x: r.left, y: r.top, width: r.width, height: r.height } }; })()`);
      if (status) await s.screenshot(`status-bar-${t}`, pad(status.rect, 24, 4));
      // The Overseer view's tab: the full-colour mark.
      const tab = await cdp.evalWorkbench(`(() => { const t = [...document.querySelectorAll('.tabs-container .tab')].find(t => t.offsetParent && /Overseer/.test(t.getAttribute('aria-label') || '')); if (!t) return null;
        const img = [t, ...t.querySelectorAll('*')].flatMap(e => [null, '::before', '::after'].map(p => getComputedStyle(e, p)).flatMap(c => [c.backgroundImage, c.webkitMaskImage])).find(b => /url\\(/.test(b || '')) || ''; const r = t.getBoundingClientRect();
        return { label: t.getAttribute('aria-label'), image: (img.match(/url\\("?(.*?)"?\\)/) || [])[1]?.split('/').pop(), rect: { x: r.left, y: r.top, width: r.width, height: r.height } }; })()`);
      if (tab) await s.screenshot(`tab-${t}`, pad(tab.rect, 40, 6));
      // The composer's heading.
      await cdp.command('Overseer: New Agent'); await delay(1500);
      const dash = await s.editorView(`document.body.dataset.mode === 'composer'`).catch(() => null);
      const hero = dash && await dash.eval(MARK_PROBE('.view-composer .hero-mark'));
      const heroRect = dash && await frameRect(dash, '.view-composer .composer-hero');
      if (heroRect) await s.screenshot(`composer-heading-${t}`, pad({ ...heroRect, height: Math.min(heroRect.height, 150) }, 40, 24));
      await s.screenshot(`window-${t}`);
      seen[theme] = { bar, status, tab, hero };
    }
    result.seen = seen;
    const all = f => THEMES.every(t => { try { return f(seen[t]); } catch { return false; } });
    check('activity bar: the Overseer icon is the single-colour mark (overseer.svg), in Overseer Dark, Overseer Light, High Contrast and Overseer', all(v => v.bar.mask === 'overseer.svg'), Object.fromEntries(THEMES.map(t => [t, seen[t].bar])));
    check('status bar: "Overseer N active" leads with the mark from the contributed icon font, not the eye', all(v => v.status.glyph && v.status.content && v.status.content !== 'none' && !v.status.eye), Object.fromEntries(THEMES.map(t => [t, seen[t].status])));
    check('the Overseer view\'s tab shows the full-colour mark (overseer-logo.png)', all(v => v.tab.image === 'overseer-logo.png'), Object.fromEntries(THEMES.map(t => [t, seen[t].tab])));
    check('the composer\'s "What\'s next?" heading shows the full-colour mark, loaded within the webview\'s Content-Security-Policy', all(v => v.hero.image === 'overseer-logo.png' && v.hero.loaded === '128x128' && v.hero.label === 'Overseer' && !v.hero.svg && !v.hero.eye), Object.fromEntries(THEMES.map(t => [t, seen[t].hero])));
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { try { await s.quit(); } catch {} s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
