const vscode = require('vscode');
const path = require('path');
const fs = require('fs').promises;
const { createHash } = require('crypto');

const MAX_PREVIEW_BYTES = 2 * 1024 * 1024;
const digest = value => createHash('sha256').update(value).digest('hex');
const keyOf = uri => uri.toString();
const contains = (root, uri) => uri.scheme === 'file' && !path.relative(root.fsPath, uri.fsPath).split(path.sep).includes('..');
async function mapLimited(items, fn, limit = 12) {
  const results = new Array(items.length);
  let next = 0;
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, async () => {
    while (next < items.length) { const i = next++; results[i] = await fn(items[i]); }
  }));
  return results;
}

/** One repository's authoritative, versioned comparison. Git and filesystem reads only. */
class Comparison {
  constructor(repo, options, helpers) {
    this.repo = repo;
    this.helpers = helpers;
    this.mode = options.mode;
    this.target = options.target;
    this.epoch = 0;
    this.version = 0;
    this.needsStatus = true;
    this.disposed = false;
    this.cache = new Map();
    this.cacheBytes = 0;
    this.emitter = new vscode.EventEmitter();
    this.onDidChange = this.emitter.event;
    this.progressEmitter = new vscode.EventEmitter();
    this.onDidProgress = this.progressEmitter.event;
    this.subscriptions = [this.emitter, this.progressEmitter, repo.state.onDidChange(() => {
      if (!this.refreshingStatus) this.invalidate(false);
    })];
    const watcher = vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(repo.rootUri, '**/*'));
    this.subscriptions.push(watcher, watcher.onDidChange(uri => this.invalidate(true, uri)),
      watcher.onDidCreate(uri => this.invalidate(true, uri)), watcher.onDidDelete(uri => this.invalidate(true, uri)),
      vscode.workspace.onDidChangeTextDocument(e => {
        if (contains(repo.rootUri, e.document.uri)) this.invalidate(false, e.document.uri);
      }),
      vscode.workspace.onDidCloseTextDocument(doc => {
        if (contains(repo.rootUri, doc.uri)) this.invalidate(true, doc.uri);
      }),
      vscode.workspace.onDidChangeConfiguration(e => {
        if (e.affectsConfiguration('overseer.review', repo.rootUri) || e.affectsConfiguration('git', repo.rootUri)) this.invalidate(true);
      }));
  }

  configure(mode, target) {
    if (this.mode === mode && this.target === target) return;
    this.mode = mode; this.target = target; this.invalidate(true);
  }

  inputs() {
    return JSON.stringify([this.mode, this.target, this.repo.state.HEAD, this.repo.state.refs,
      this.target]);
  }

  progress(stage, uri) {
    this.stage = stage;
    this.progressEmitter.fire({ stage, id: uri ? digest(keyOf(uri)) : undefined, epoch: this.epoch });
  }

  invalidate(status = false, uri) {
    if (this.disposed) return;
    this.epoch++;
    this.needsStatus ||= status;
    if (uri) this.progress('Updating comparison…', uri);
    clearTimeout(this.timer);
    this.timer = setTimeout(() => { this.ready().catch(() => {}); }, 150);
  }

  get display() { return this.preview || this.snapshot; }

  async ready() {
    if (this.disposed) throw new Error('The comparison has been closed.');
    if (this.running) return this.running;
    if (this.snapshot && this.appliedEpoch === this.epoch) return this.snapshot;
    clearTimeout(this.timer);
    this.running = this.compute();
    try { return await this.running; } finally { this.running = undefined; }
  }

  async compute() {
    while (!this.disposed) {
      const epoch = this.epoch;
      let next;
      try {
        this.progress(this.snapshot ? 'Updating comparison…' : 'Finding branch base…');
        if (this.needsStatus) {
          this.needsStatus = false;
          this.refreshingStatus = true;
          try { await this.repo.status(); } finally { this.refreshingStatus = false; }
        }
        const head = this.repo.state.HEAD?.commit;
        const key = await this.helpers.comparisonKey(this.repo, this.target);
        let context;
        if (this.baseCache?.key === key) context = this.baseCache.context;
        else {
          context = await this.helpers.resolveContext(this.target, this.repo);
          // getBranchBase may itself cache its answer in Git config.
          const resolvedKey = await this.helpers.comparisonKey(this.repo, this.target);
          this.baseCache = resolvedKey === key ? { key, context } : undefined;
        }
        this.progress(this.snapshot ? 'Updating comparison…' : 'Finding changed files…');
        const inputs = this.inputs();
        const description = this.baseCache?.description || { base: context.base, mergeBase: context.mergeBase,
          headName: this.repo.state.HEAD?.name, headSha: head, mode: this.mode };
        const partial = { context, description: { ...description, mode: this.mode }, inputs,
          warning: this.completenessWarning(), mode: this.mode, checking: true };
        const publish = (entries, discovery = false) => {
          // A fast refresh with identical paths can retain its validated list.
          const old = this.snapshot;
          const membership = list => JSON.stringify(list.map(e => [e.id, e.status, !!e.unsaved]));
          if (discovery && old && !this.preview && old.mode === partial.mode && old.context?.mergeBase === context.mergeBase &&
              old.description?.headName === description.headName && old.description?.base === description.base &&
              membership(old.entries) === membership(entries)) return;
          if (this.disposed || epoch !== this.epoch || inputs !== this.inputs()) return;
          this.preview = { ...partial, entries, version: ++this.version };
          this.emitter.fire(this.preview);
        };
        const entries = await this.entries(context, head, publish, () => epoch === this.epoch);
        if (this.repo.state.HEAD?.commit !== head || inputs !== this.inputs()) { this.epoch++; continue; }
        next = { context, description: { ...description, mode: this.mode }, entries, inputs,
          warning: this.completenessWarning(), mode: this.mode };
      } catch (error) {
        next = { entries: [], error: error.message || String(error), mode: this.mode };
      }
      if (epoch !== this.epoch) continue;
      const fingerprint = digest(JSON.stringify({
        inputs: next.inputs, description: next.description, warning: next.warning, error: next.error, mode: next.mode,
        entries: next.entries.map(e => [e.id, e.revision, e.status, e.unsaved, e.problem]),
      }));
      this.appliedEpoch = epoch;
      if (fingerprint !== this.fingerprint || this.preview) {
        this.preview = undefined;
        this.fingerprint = fingerprint;
        this.snapshot = { ...next, version: ++this.version };
        this.emitter.fire(this.snapshot);
      }
      this.progress('');
      const published = this.snapshot;
      // Commit subjects are decorative: publish the authoritative membership first.
      if (published.context && !this.baseCache?.description) {
        const cache = this.baseCache;
        this.helpers.describeComparison(this.repo, published.context.base, published.context.mergeBase, published.mode).then(description => {
          if (this.disposed || this.snapshot !== published || this.inputs() !== published.inputs || epoch !== this.epoch) return;
          if (cache && this.baseCache === cache) cache.description = description;
          published.description = description;
          this.emitter.fire(published);
        }).catch(() => {});
      }
      return published;
    }
    throw new Error('The comparison has been closed.');
  }

  completenessWarning() {
    if (this.mode !== 'workingTree') return undefined;
    const config = vscode.workspace.getConfiguration('git', this.repo.rootUri);
    if (config.get('untrackedChanges') === 'hidden') return 'Incomplete comparison: git.untrackedChanges is hidden. Use separate or mixed to include untracked files.';
    const limit = config.get('statusLimit', 10000);
    const state = this.repo.state;
    const count = ['indexChanges', 'workingTreeChanges', 'untrackedChanges', 'mergeChanges']
      .reduce((total, key) => total + (state[key]?.length || 0), 0);
    if (limit > 0 && count >= limit) return `Git may have truncated status at ${limit} files. Increase git.statusLimit (0 means unlimited) before treating this comparison as complete.`;
    return undefined;
  }

  async entries(context, head, publish, current) {
    const entries = await this.helpers.getChangeEntries(context.git, this.repo, this.mode, context.mergeBase);
    const byPath = new Map();
    for (const entry of entries) {
      const key = keyOf(entry.uri), previous = byPath.get(key);
      // A staged deletion recreated as untracked retains its original base endpoint.
      byPath.set(key, previous ? { ...previous, right: entry.right || previous.right, status: previous.left ? 5 : entry.status } : { ...entry });
    }
    const ordered = () => [...byPath.values()].sort((a, b) => a.relPath.localeCompare(b.relPath));
    const pending = entry => ({ ...entry, id: digest(keyOf(entry.uri)), pending: true });
    for (const [key, entry] of byPath) byPath.set(key, pending(entry));
    // Git returns a complete array. Publish its paths before per-file validation.
    publish(ordered(), true);
    this.progress('Checking files…');
    if (this.mode === 'workingTree') {
      const dirty = vscode.workspace.textDocuments.filter(d => d.isDirty && contains(this.repo.rootUri, d.uri));
      const ignored = dirty.length ? await this.repo.checkIgnore(dirty.map(d => d.uri.fsPath)) : new Set();
      for (const doc of dirty) {
        let entry = byPath.get(keyOf(doc.uri));
        if (!entry) {
          if (ignored.has(doc.uri.fsPath)) continue;
          entry = pending({ uri: doc.uri, relPath: path.relative(this.repo.rootUri.fsPath, doc.uri.fsPath).split(path.sep).join('/'),
            status: 5, left: null, right: doc.uri, findBase: true });
        }
        byPath.set(keyOf(doc.uri), { ...entry, unsaved: true, documentVersion: doc.version, right: doc.uri, dirtyText: doc.getText() });
      }
      if (dirty.length) publish(ordered(), true);
    }
    let timer;
    try {
      await mapLimited(ordered(), async entry => {
        if (!current()) return;
        const validated = await this.validateEntry(entry, context, head);
        if (!current()) return;
        if (validated) byPath.set(keyOf(entry.uri), validated); else byPath.delete(keyOf(entry.uri));
        // Coalesce validation results; never hold the usable list behind one slow file.
        if (!timer) timer = setTimeout(() => { timer = undefined; publish(ordered()); }, 40);
      }, 4);
    } finally { clearTimeout(timer); }
    return ordered();
  }

  async validateEntry(candidate, context, head) {
    const entry = { ...candidate, pending: false };
    if (entry.unsaved) {
      try {
        if (entry.findBase) {
          const details = await this.objectDetails(context.mergeBase, entry.uri.fsPath);
          entry.left = details ? context.git.toGitUri(entry.uri, context.mergeBase) : null;
          entry.status = details ? 5 : 7;
          delete entry.findBase;
        }
        const base = entry.left ? await this.blob(entry.left) : Buffer.alloc(0);
        if (base.toString('utf8') === entry.dirtyText && entry.left && entry.status !== 3) return null;
      } catch (error) { entry.problem = error.message; }
    }
    let stamp = '';
    if (this.mode === 'committed') {
      if (entry.right) entry.right = context.git.toGitUri(entry.uri, head);
      stamp = head;
    } else if (entry.right && !entry.unsaved) {
      try {
        const stat = await fs.lstat(entry.uri.fsPath);
        stamp = [stat.size, stat.mtimeMs, stat.ctimeMs, stat.mode].join(':');
        if (stat.isSymbolicLink()) entry.symbolicLink = true;
        else if (!stat.isFile()) entry.problem = 'Directory or submodule change — open in the native view.';
      } catch (error) {
        if (error.code === 'ENOENT') { entry.right = null; entry.status = 2; stamp = 'missing'; }
        else entry.problem = `Cannot read file: ${error.message}`;
      }
    }
    entry.stamp = stamp;
    entry.revision = digest(JSON.stringify([entry.left?.toString(), entry.right?.toString(), stamp, entry.documentVersion, entry.dirtyText, entry.problem]));
    return entry.left || entry.right ? entry : null;
  }

  async objectDetails(ref, file) {
    try { return await this.repo.getObjectDetails(ref, file); }
    catch (error) { if (error.gitErrorCode === 'UnknownPath' || error.message === 'Path not known by git') return null; throw error; }
  }

  async blob(uri) {
    const key = keyOf(uri);
    const cached = this.cache.get(key);
    if (cached) return cached;
    const ref = JSON.parse(uri.query).ref;
    const details = await this.objectDetails(ref, uri.fsPath);
    if (!details) throw new Error('The base file is no longer available. Refresh the comparison.');
    if (details.mode === '160000') throw new Error('Submodule change — open in the native view.');
    if (details.size > MAX_PREVIEW_BYTES) throw new Error('File exceeds the 2 MiB preview limit. Open in Native Diff to inspect it.');
    const content = await this.repo.buffer(ref, uri.fsPath);
    this.cache.set(key, content); this.cacheBytes += content.length;
    while (this.cache.size > 80 || this.cacheBytes > 16 * 1024 * 1024) {
      const oldest = this.cache.keys().next().value;
      this.cacheBytes -= this.cache.get(oldest).length; this.cache.delete(oldest);
    }
    return content;
  }

  async validEntry(snapshot, entry) {
    if (this.disposed || this.inputs() !== snapshot.inputs) return false;
    const current = this.display?.entries.find(e => e.id === entry.id);
    if (!current || current.pending || current.revision !== entry.revision) return false;
    if (snapshot.mode !== 'workingTree') return true;
    const doc = vscode.workspace.textDocuments.find(d => keyOf(d.uri) === keyOf(entry.uri));
    if (entry.unsaved) return !!doc?.isDirty && doc.version === entry.documentVersion;
    if (doc?.isDirty) return false;
    if (!entry.right && !entry.problem) {
      try { await fs.lstat(entry.uri.fsPath); return false; }
      catch (error) { return error.code === 'ENOENT'; }
    }
    if (entry.right && !entry.problem) {
      try {
        const stat = await fs.lstat(entry.uri.fsPath);
        return entry.stamp === [stat.size, stat.mtimeMs, stat.ctimeMs, stat.mode].join(':');
      } catch { return false; }
    }
    return true;
  }

  async body(id, version, revision) {
    // An unrelated refresh must not hold a validated file hostage.
    const snapshot = this.display;
    if (!snapshot || (!revision && snapshot.version !== version)) return undefined;
    const entry = snapshot.entries.find(e => e.id === id);
    if (!entry || entry.pending || (revision && revision !== entry.revision) || !await this.validEntry(snapshot, entry)) return undefined;
    let body;
    try {
      if (entry.problem) throw new Error(entry.problem);
      const original = entry.left ? await this.blob(entry.left) : Buffer.alloc(0);
      let modified = Buffer.alloc(0);
      if (entry.right) {
        if (entry.unsaved) modified = Buffer.from(entry.dirtyText, 'utf8');
        else if (entry.right.scheme === 'git') modified = await this.blob(entry.right);
        else if (entry.symbolicLink) modified = Buffer.from(await fs.readlink(entry.uri.fsPath));
        else {
          const stat = await fs.stat(entry.uri.fsPath);
          if (stat.size > MAX_PREVIEW_BYTES) throw new Error('File exceeds the 2 MiB preview limit. Open in Native Diff to inspect it.');
          modified = await fs.readFile(entry.uri.fsPath);
        }
      }
      if (Math.max(original.length, modified.length) > MAX_PREVIEW_BYTES) throw new Error('File exceeds the 2 MiB preview limit. Open in Native Diff to inspect it.');
      if (original.includes(0) || modified.includes(0)) throw new Error('Binary file — open in the native view.');
      const decode = data => new TextDecoder('utf-8', { fatal: true }).decode(data);
      body = { original: decode(original), modified: decode(modified) };
    } catch (error) { body = { problem: error.message || String(error) }; }
    if (!await this.validEntry(snapshot, entry)) return undefined;
    return { ...body, id, version, revision: entry.revision };
  }

  dispose() {
    this.disposed = true; this.epoch++; clearTimeout(this.timer);
    this.subscriptions.forEach(d => d.dispose()); this.cache.clear(); this.cacheBytes = 0;
  }
}
module.exports = { Comparison, digest, contains, mapLimited };
