const vscode = require('vscode');
const fs = require('fs').promises;
const path = require('path');
const { digest, contains } = require('./comparison');

const LIMIT = 2 * 1024 * 1024;
const stamp = stat => [stat.size, stat.mtimeMs, stat.ctimeMs, stat.mode].join(':');
const textValid = text => typeof text === 'string' && !text.includes('\0') && Buffer.byteLength(text, 'utf8') <= LIMIT;

/** Writes belong to real VS Code documents, independently of a webview's lifetime. */
class Editing {
  constructor(context) {
    this.context = context;
    this.streams = new Map();
    this.generations = new Map();
    this.queues = new Map();
    this.pending = new Map();
    this.documents = new Map();
    this.backups = new Map();
    this.storage = context.storageUri && vscode.Uri.joinPath(context.storageUri, 'edit-recovery');
    const changed = doc => {
      const record = this.documents.get(doc.uri.toString());
      if (!record) return;
      const operation = this.queues.get(record.repository + ':' + record.id);
      Promise.resolve(operation).catch(() => {}).then(() => this.backup(record)).catch(error =>
        vscode.window.showErrorMessage('Overseer review could not retain the unsaved recovery copy: ' + error.message));
    };
    context.subscriptions.push(vscode.workspace.onDidChangeTextDocument(e => changed(e.document)),
      vscode.workspace.onDidSaveTextDocument(changed),
      vscode.workspace.onDidCloseTextDocument(doc => this.documents.delete(doc.uri.toString())));
  }

  enabled(session) {
    return session.mode === 'workingTree' && vscode.workspace.getConfiguration('overseer.review', session.repo.rootUri).get('enableEditing', true);
  }

  async writable(session, uri) {
    if (!contains(session.repo.rootUri, uri) || uri.toString() === session.repo.rootUri.toString()) throw new Error('File is outside this review.');
    const relative = path.relative(session.repo.rootUri.fsPath, uri.fsPath);
    if (relative.split(path.sep).some(part => part.toLowerCase() === '.git')) throw new Error('Git metadata cannot be edited in the review.');
    const stat = await fs.lstat(uri.fsPath);
    if (!stat.isFile() || stat.isSymbolicLink()) throw new Error('This resource requires the native editor.');
    const realRoot = await fs.realpath(session.repo.rootUri.fsPath), realFile = await fs.realpath(uri.fsPath);
    const realRelative = path.relative(realRoot, realFile);
    if (realRelative !== relative || realRelative.split(path.sep).includes('..')) throw new Error('Linked files require the native editor.');
    const info = await vscode.workspace.fs.stat(uri);
    if (info.permissions === vscode.FilePermission.Readonly || !(stat.mode & 0o222) || vscode.workspace.fs.isWritableFileSystem(uri.scheme) === false) throw new Error('File is read-only.');
    const config = vscode.workspace.getConfiguration('files', uri);
    // VS Code's resource filters use glob expressions. RelativePattern delegates
    // matching to VS Code instead of introducing another glob implementation.
    for (const [glob, enabled] of Object.entries(config.get('readonlyInclude', {}))) {
      if (!enabled || !await this.matches(session, uri, glob)) continue;
      const excluded = await Promise.all(Object.entries(config.get('readonlyExclude', {})).filter(([, on]) => on).map(([pattern]) => this.matches(session, uri, pattern)));
      if (!excluded.some(Boolean)) throw new Error('File is configured read-only.');
    }
    return stamp(stat);
  }

  async matches(session, uri, glob) {
    const files = await vscode.workspace.findFiles(new vscode.RelativePattern(session.repo.rootUri, glob), null);
    return files.some(file => file.toString() === uri.toString());
  }

  receive(session, message, send) {
    if (!['edit', 'save', 'recover', 'endEdit'].includes(message.type)) return false;
    if (!/^[a-f0-9]{64}$/.test(message.id || '') || message.repository !== session.repo.rootUri.toString() ||
        !/^[a-zA-Z0-9-]{1,80}$/.test(message.stream || '') || !Number.isSafeInteger(message.sequence) || message.sequence < 1) return true;
    message = { ...message, generation: this.generations.get(message.repository) || 0 };
    const key = session.repo.rootUri.toString() + ':' + message.id;
    const item = { session, message, send };
    if (this.pending.has(key)) { this.pending.get(key).push(item); return true; }
    const waiting = [item]; this.pending.set(key, waiting);
    const operation = Promise.resolve().then(async () => {
      while (waiting.length) {
        const next = waiting.shift(), batch = [next.message];
        // A paste/IME may emit hundreds of incremental events. Compose only
        // consecutive operations from the same validated stream, then journal
        // and apply once; unrelated files retain independent queues.
        while (next.message.type === 'edit' && waiting[0]?.message.type === 'edit' &&
          waiting[0].message.stream === next.message.stream && waiting[0].message.revision === next.message.revision &&
          waiting[0].message.sequence === batch.at(-1).sequence + 1) batch.push(waiting.shift().message);
        try { await this.process(next.session, next.message, next.send, batch); }
        catch (error) { next.send({ type: 'editRejected', id: next.message.id, stream: next.message.stream,
          sequence: batch.at(-1).sequence, message: error.message }); }
      }
    });
    this.queues.set(key, operation);
    operation.finally(() => { this.pending.delete(key); if (this.queues.get(key) === operation) this.queues.delete(key); });
    return true;
  }

  async journal(stream) {
    if (!this.storage) throw new Error('Local draft recovery storage is unavailable.');
    await vscode.workspace.fs.createDirectory(this.storage);
    const file = vscode.Uri.joinPath(this.storage, digest(stream.repository + ':' + stream.id + ':' + stream.token) + '.json');
    const temporary = file.with({ path: file.path + '.tmp' });
    await vscode.workspace.fs.writeFile(temporary, Buffer.from(JSON.stringify({ repository: stream.repository, id: stream.id,
      uri: stream.uri?.toString(), before: stream.before, text: stream.text, token: stream.token })));
    await vscode.workspace.fs.rename(temporary, file, { overwrite: true });
    stream.journal = file;
  }

  async clearJournal(stream) {
    if (stream.journal) { await vscode.workspace.fs.delete(stream.journal).catch(() => {}); stream.journal = undefined; }
  }

  async backup(record) {
    const doc = record.doc;
    if (doc.isClosed) return;
    const file = vscode.Uri.joinPath(this.storage, 'accepted-' + record.id + '.json');
    const data = doc.isDirty ? JSON.stringify({ accepted: true, repository: record.repository, id: record.id,
      uri: doc.uri.toString(), before: '', text: doc.getText(), token: record.token, baseline: record.baseline }) : undefined;
    // Each real document has one handoff record, even when its offscreen Monaco
    // model is destroyed and recreated. Acknowledgements wait for this write.
    const previous = this.backups.get(record.id) || Promise.resolve();
    const writing = previous.catch(() => {}).then(async () => {
      if (data === undefined) await vscode.workspace.fs.delete(file).catch(() => {});
      else {
        if (!textValid(JSON.parse(data).text)) throw new Error('Unsaved content exceeds the recovery limit. Save it in the native editor.');
        await vscode.workspace.fs.createDirectory(this.storage);
        const temporary = file.with({ path: file.path + '.tmp' });
        await vscode.workspace.fs.writeFile(temporary, Buffer.from(data));
        await vscode.workspace.fs.rename(temporary, file, { overwrite: true });
      }
    });
    this.backups.set(record.id, writing);
    try { await writing; } finally { if (this.backups.get(record.id) === writing) this.backups.delete(record.id); }
    if (!doc.isDirty && this.documents.get(doc.uri.toString()) === record) this.documents.delete(doc.uri.toString());
  }

  async handoff(stream) {
    const previous = this.documents.get(stream.uri.toString());
    const record = { ...stream, baseline: previous?.baseline || stream.baseline };
    this.documents.set(stream.uri.toString(), record);
    await this.backup(record);
  }

  async restoreAccepted(session, draft, file) {
    if (!file || !/^[a-f0-9]{64}$/.test(draft.baseline || '')) return false;
    const diskStamp = await this.writable(session, file);
    const bytes = await fs.readFile(file.fsPath);
    const doc = await vscode.workspace.openTextDocument(file);
    if (doc.getText() !== draft.text) {
      // A newer dirty buffer or changed disk always wins. Restoration changes
      // only a validated native buffer, never the file on disk.
      if (doc.isDirty || digest(bytes) !== draft.baseline || doc.getText() !== new TextDecoder('utf-8', { fatal: true }).decode(bytes)) return false;
      const version = doc.version;
      if (diskStamp !== await this.writable(session, file) || doc.version !== version) return false;
      const edit = new vscode.WorkspaceEdit();
      edit.replace(file, new vscode.Range(doc.positionAt(0), doc.positionAt(doc.getText().length)), draft.text);
      if (!await vscode.workspace.applyEdit(edit) || doc.getText() !== draft.text) return false;
    }
    await this.handoff({ ...draft, uri: file, doc, session });
    session.invalidate(false, file);
    return true;
  }

  async start(session, message) {
    if (!this.enabled(session) || !textValid(message.before)) throw new Error('Editing is unavailable for this comparison.');
    const snapshot = session.display;
    const entry = snapshot?.entries.find(e => e.id === message.id);
    if (!entry || entry.pending || !entry.right || entry.problem || entry.symbolicLink || entry.revision !== message.revision || !await session.validEntry(snapshot, entry)) throw new Error('The file changed before editing began. Your draft has been retained.');
    const body = await session.body(entry.id, snapshot.version, entry.revision);
    if (!body || body.problem || body.modified !== message.before) throw new Error(body?.problem || 'The preview changed before editing began. Your draft has been retained.');
    const diskStamp = await this.writable(session, entry.uri);
    const baseline = digest(await fs.readFile(entry.uri.fsPath));
    const doc = await vscode.workspace.openTextDocument(entry.uri);
    if (doc.getText() !== message.before || !textValid(doc.getText()) || snapshot.inputs !== session.inputs() || diskStamp !== await this.writable(session, entry.uri)) throw new Error('The file changed before editing began. Your draft has been retained.');
    return { token: message.stream, repository: message.repository, id: message.id, uri: entry.uri, doc,
      version: doc.version, sequence: 0, text: doc.getText(), before: doc.getText(), baseline, diskStamp, inputs: session.inputs(), session };
  }

  patch(before, changes) {
    if (!Array.isArray(changes) || !changes.length || changes.length > 10000) throw new Error('Invalid edit operations.');
    const ordered = [...changes].sort((a, b) => b.offset - a.offset);
    let inserted = 0;
    for (const edit of ordered) {
      if (!textValid(edit.text) || (inserted += Buffer.byteLength(edit.text, 'utf8')) > LIMIT) throw new Error('Edit exceeds the 2 MiB preview limit. Use Open in Native Diff.');
    }
    let end = before.length, result = before;
    for (const edit of ordered) {
      if (!Number.isSafeInteger(edit.offset) || !Number.isSafeInteger(edit.length) || edit.offset < 0 || edit.length < 0 ||
          edit.offset + edit.length > end || !textValid(edit.text)) throw new Error('Invalid edit range.');
      result = result.slice(0, edit.offset) + edit.text + result.slice(edit.offset + edit.length);
      end = edit.offset;
    }
    if (!textValid(result)) throw new Error('Edit exceeds the 2 MiB preview limit. Use Open in Native Diff.');
    return { text: result, changes: ordered };
  }

  async process(session, message, send, operations = [message]) {
    const token = message.repository + ':' + message.id + ':' + message.stream;
    let stream = this.streams.get(token);
    if (message.type === 'endEdit') { this.streams.delete(token); return; }
    if (message.type === 'recover') {
      // Recovery never writes a repository file. The user gets a real untitled
      // document so even an obsolete branch draft can be copied/resolved safely.
      if (!textValid(message.draft)) throw new Error('Invalid recovery draft.');
      const doc = await vscode.workspace.openTextDocument({ content: message.draft, language: stream?.doc?.languageId || 'plaintext' });
      if (stream?.uri && await fs.stat(stream.uri.fsPath).then(s => s.isFile(), () => false)) {
        await vscode.commands.executeCommand('vscode.diff', stream.uri, doc.uri, 'Recovered review draft', { preview: false });
      } else await vscode.window.showTextDocument(doc, { preview: false });
      // Opening a native tab can recreate the webview before it receives the
      // acknowledgement. Keep bounded content-free handoff receipts so its old
      // saved draft does not incorrectly lock the file again on return.
      const receipt = digest(token + ':' + digest(message.draft));
      const receipts = this.context.workspaceState.get('editHandoffs', []);
      await this.context.workspaceState.update('editHandoffs', [...receipts.filter(value => value !== receipt), receipt].slice(-1000));
      if (stream) await this.clearJournal(stream);
      else if (this.storage) await vscode.workspace.fs.delete(vscode.Uri.joinPath(this.storage, digest(token) + '.json')).catch(() => {});
      if (this.storage) {
        const accepted = vscode.Uri.joinPath(this.storage, 'accepted-' + message.id + '.json');
        try {
          const record = JSON.parse(Buffer.from(await vscode.workspace.fs.readFile(accepted)).toString('utf8'));
          if (record.token === message.stream && record.text === message.draft) await vscode.workspace.fs.delete(accepted);
        } catch { /* There may only be an unacknowledged-operation journal. */ }
      }
      send({ type: 'editRecovered', id: message.id, stream: message.stream });
      return;
    }
    if (!stream) {
      if (message.sequence !== 1) throw new Error('This editing session has expired. Reopen the file to continue.');
      try { stream = await this.start(session, message); }
      catch (error) {
        if (!textValid(message.before)) throw error;
        stream = { token: message.stream, repository: message.repository, id: message.id, sequence: 0,
          before: message.before, text: message.before, failed: error.message };
      }
      stream.generation = message.generation;
      this.streams.set(token, stream);
    }
    if (message.sequence !== stream.sequence + 1) throw new Error('Duplicate or out-of-order edit was rejected.');
    const before = stream.text;
    let patch = { text: before, changes: [] };
    if (message.type === 'edit') {
      for (const operation of operations) patch = this.patch(patch.text, operation.changes);
      if (operations.length > 1) {
        let start = 0, end = 0;
        while (start < before.length && start < patch.text.length && before[start] === patch.text[start]) start++;
        while (end < before.length - start && end < patch.text.length - start && before[before.length-end-1] === patch.text[patch.text.length-end-1]) end++;
        patch.changes = [{ offset: start, length: before.length-start-end, text: patch.text.slice(start,patch.text.length-end) }];
      }
    }
    message = operations.at(-1);
    stream.before = before; stream.text = patch.text;
    // Journal before any asynchronous apply/save: closing the panel must not
    // cancel a received operation or discard its rejected replacement text.
    await this.journal(stream);
    try {
      if (stream.failed) throw new Error(stream.failed);
      if (!this.enabled(session) || stream.inputs !== session.inputs()) throw new Error('The branch, base or editing setting changed. Your draft has been retained.');
      const diskStamp = await this.writable(session, stream.uri);
      if (diskStamp !== stream.diskStamp) {
        const bytes = await fs.readFile(stream.uri.fsPath);
        const disk = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
        if (disk !== before || stream.doc.isDirty) throw new Error('The file changed on disk. Your draft has been retained.');
        stream.diskStamp = diskStamp; stream.baseline = digest(bytes);
      }
      if (stream.inputs !== session.inputs() || !this.enabled(session) || stream.doc.isClosed || stream.doc.version !== stream.version || stream.doc.getText() !== before) throw new Error('The file changed in another editor. Your draft has been retained.');
      if (message.type === 'edit') {
        const edit = new vscode.WorkspaceEdit();
        for (const change of patch.changes) edit.replace(stream.uri, new vscode.Range(stream.doc.positionAt(change.offset), stream.doc.positionAt(change.offset + change.length)), change.text);
        // No await between the final document-version check and applyEdit:
        // VS Code also carries that version into its main-thread bulk edit.
        if (!await vscode.workspace.applyEdit(edit) || stream.doc.getText() !== patch.text) throw new Error('The edit could not be applied safely. Your draft has been retained.');
      } else if (!await stream.doc.save()) {
        stream.sequence = message.sequence;
        await this.handoff(stream); await this.clearJournal(stream);
        send({ type: 'saveFailed', id: stream.id, stream: message.stream, sequence: stream.sequence,
          documentVersion: stream.doc.version, message: 'Save failed. Your unsaved text is retained; retry Save or use Open in Native Diff.' });
        return;
      }
      stream.version = stream.doc.version; stream.sequence = message.sequence;
      if (message.type === 'save') {
        stream.diskStamp = await this.writable(session, stream.uri);
        stream.baseline = digest(await fs.readFile(stream.uri.fsPath));
        if (stream.doc.getText() !== before) stream.failed = 'Saving changed the document (for example, formatting). Your newer review draft has been retained.';
      }
      await this.handoff(stream);
      await this.clearJournal(stream);
      send({ type: 'editAccepted', id: stream.id, stream: message.stream, sequence: stream.sequence,
        documentVersion: stream.version, unsaved: stream.doc.isDirty, saved: message.type === 'save' });
      session.invalidate(false, stream.uri);
    } catch (error) { stream.failed = error.message; stream.sequence = message.sequence; throw error; }
  }

  async recoverStored(session) {
    if (!this.storage) return [];
    const result = [];
    const files = await vscode.workspace.fs.readDirectory(this.storage).catch(() => []);
    for (const [name, type] of files.sort(([a], [b]) => b.localeCompare(a))) {
      if (type !== vscode.FileType.File || !/^(accepted-)?[a-f0-9]{64}\.json$/.test(name)) continue;
      const uri = vscode.Uri.joinPath(this.storage, name);
      try {
        const stat = await vscode.workspace.fs.stat(uri);
        if (stat.size > LIMIT * 6) continue;
        const draft = JSON.parse(Buffer.from(await vscode.workspace.fs.readFile(uri)).toString('utf8'));
        if (draft.repository !== session.repo.rootUri.toString() || !textValid(draft.text) || !textValid(draft.before) || !/^[a-f0-9]{64}$/.test(draft.id)) continue;
        const file = draft.uri && vscode.Uri.parse(draft.uri);
        if (file && (!contains(session.repo.rootUri, file) || digest(file.toString()) !== draft.id)) continue;
        if (!/^[a-zA-Z0-9-]{1,80}$/.test(draft.token || '')) continue;
        if (draft.accepted && await this.restoreAccepted(session, draft, file).catch(() => false)) continue;
        const existing = file && vscode.workspace.textDocuments.find(doc => doc.uri.toString() === file.toString());
        if (!draft.accepted && existing?.getText() === draft.text) { await vscode.workspace.fs.delete(uri); continue; }
        result.push({ id: draft.id, stream: draft.token, draft: draft.text, before: draft.before, path: file ? path.relative(session.repo.rootUri.fsPath, file.fsPath) : undefined });
      } catch { /* A corrupt draft cannot authorize a repository write. */ }
    }
    return result;
  }

  async handoffDrafts(session, drafts) {
    const handedOff = [];
    if (!Array.isArray(drafts) || drafts.length > 10000) return handedOff;
    for (const draft of drafts) {
      if (!draft || draft.repository !== session.repo.rootUri.toString() || !textValid(draft.draft) ||
        !/^[a-f0-9]{64}$/.test(draft.id || '') || !/^[a-zA-Z0-9-]{1,80}$/.test(draft.stream || '')) continue;
      const pending = this.queues.get(draft.repository + ':' + draft.id);
      if (pending) await pending.catch(() => {});
      const token = draft.repository + ':' + draft.id + ':' + draft.stream;
      if (this.context.workspaceState.get('editHandoffs', []).includes(digest(token + ':' + digest(draft.draft)))) {
        handedOff.push({ id: draft.id, stream: draft.stream }); continue;
      }
      const doc = vscode.workspace.textDocuments.find(doc => contains(session.repo.rootUri, doc.uri) &&
        digest(doc.uri.toString()) === draft.id && doc.getText() === draft.draft);
      if (!doc) continue;
      try {
        const diskStamp = await this.writable(session, doc.uri);
        const baseline = digest(await fs.readFile(doc.uri.fsPath));
        if (doc.getText() !== draft.draft || diskStamp !== await this.writable(session, doc.uri)) continue;
        await this.handoff({ repository: draft.repository, id: draft.id, token: draft.stream, uri: doc.uri, doc, baseline });
        handedOff.push({ id: draft.id, stream: draft.stream });
      } catch { /* Keep the draft visible unless its native handoff is proven. */ }
    }
    return handedOff;
  }

  close(session) {
    const repository = session.repo.rootUri.toString(), generation = this.generations.get(repository) || 0;
    this.generations.set(repository, generation + 1);
    const pending = [...this.queues].filter(([key]) => key.startsWith(session.repo.rootUri.toString() + ':')).map(([, promise]) => promise);
    Promise.allSettled(pending).then(() => {
      for (const [key, stream] of this.streams) if (stream.repository === repository && stream.generation <= generation) this.streams.delete(key);
    });
  }
}
module.exports = { Editing, LIMIT };
