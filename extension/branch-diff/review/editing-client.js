const LIMIT = 2 * 1024 * 1024;
const byteLength = text => new TextEncoder().encode(text).length;

// A minimal replacement preserves Monaco selections without resetting models.
export function replaceText(model, text) {
  const old = model.getValue();
  if (old === text) return;
  let start = 0, end = 0;
  while (start < old.length && start < text.length && old[start] === text[start]) start++;
  while (end < old.length - start && end < text.length - start && old[old.length - end - 1] === text[text.length - end - 1]) end++;
  const from = model.getPositionAt(start), to = model.getPositionAt(old.length - end);
  model.applyEdits([{ range: { startLineNumber: from.lineNumber, startColumn: from.column,
    endLineNumber: to.lineNumber, endColumn: to.column }, text: text.slice(start, text.length - end) }]);
}

export class EditingClient {
  constructor(api) {
    this.api = api; this.rows = new Map(); this.drafts = new Map();
    for (const draft of api.saved.drafts || []) if (draft.repository === api.repository && /^[a-f0-9]{64}$/.test(draft.id) &&
      typeof draft.draft === 'string' && byteLength(draft.draft) <= LIMIT) this.drafts.set(draft.id, draft);
    this.bar = document.createElement('div'); this.bar.className = 'edit-recovery'; this.bar.setAttribute('role', 'status');
    document.getElementById('notice').after(this.bar); this.renderRecovery();
    document.addEventListener('keydown', e => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const row = this.focused(); if (!row) return;
      if (['z', 'y'].includes(e.key.toLowerCase())) { e.preventDefault(); e.stopImmediatePropagation(); this.undoNotice(); }
      else if (e.key.toLowerCase() === 's') { e.preventDefault(); e.stopImmediatePropagation(); this.save(row); }
    }, true);
    document.addEventListener('beforeinput', e => {
      if (this.focused() && ['historyUndo', 'historyRedo'].includes(e.inputType)) { e.preventDefault(); e.stopImmediatePropagation(); this.undoNotice(); }
    }, true);
  }

  undoNotice() { this.api.notice('Undo/redo is available through Open in Native Diff. Review edits remain in the same VS Code document.'); }
  focused() { return [...this.rows.values()].find(row => row.editor?.getModifiedEditor().hasTextFocus()); }
  held(row) { return !!row.edit && (row.edit.sequence > row.edit.ack || !!row.edit.failed); }
  pending(row) { return !!row.edit && row.edit.sequence > row.edit.ack && !row.edit.failed; }
  state() { return [...this.drafts.values()]; }
  persist() { this.api.vscode.setState({ ...(this.api.vscode.getState() || {}), drafts: this.state() }); }
  enabled(row) {
    const draft = this.drafts.get(row.entry.id);
    return this.api.settings().enableEditing !== false && !!row.body?.editable && !row.edit?.failed &&
      (!draft || row.edit?.stream === draft.stream) && !this.api.snapshot()?.cached && this.api.snapshot()?.mode === 'workingTree';
  }

  update(row) {
    const enabled = this.enabled(row);
    if (row.editor && row.editable !== enabled) { row.editor.updateOptions({ readOnly: !enabled, domReadOnly: !enabled, originalEditable: false }); row.editable = enabled; }
    row.save.disabled = !this.enabled(row) || !!row.edit?.saving;
    row.save.hidden = this.api.snapshot()?.mode !== 'workingTree';
    row.editStatus.textContent = row.edit?.failed ? 'Draft needs attention' : row.edit?.saveError ? 'Save failed' : row.edit?.saving ? 'Saving…' : this.pending(row) ? 'Syncing…' : '';
    row.editStatus.title = row.edit?.failed || row.edit?.saveError || '';
    row.element.dataset.editState = row.edit?.failed ? 'conflict' : this.pending(row) ? 'pending' : 'ready';
  }

  attach(row, monaco) {
    this.rows.set(row.entry.id, row);
    row.editText = row.modified.getValue();
    const editor = row.editor.getModifiedEditor();
    // Monaco's local undo would diverge from the authoritative document history.
    row.listeners.push(editor.addAction({ id: 'branch-diff-no-undo', label: 'Undo in Native Diff', keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyZ], run: () => this.undoNotice() }),
      editor.addAction({ id: 'branch-diff-no-redo', label: 'Redo in Native Diff', keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyZ, monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyY], run: () => this.undoNotice() }),
      editor.onDidBlurEditorText(() => { this.api.changed(row); }),
      row.modified.onDidChangeContent(event => this.changed(row, event)));
    this.update(row);
  }

  stream(row) {
    if (!row.edit) row.edit = { stream: crypto.randomUUID(), sequence: 0, ack: 0, before: row.editText,
      version: this.api.snapshot().version, revision: row.body.revision, repository: this.api.repository };
    return row.edit;
  }

  changed(row, event) {
    if (row.applying) return;
    if (event.isUndoing || event.isRedoing) {
      row.applying = true; replaceText(row.modified, row.editText); row.applying = false; this.undoNotice(); return;
    }
    const text = row.modified.getValue();
    if (!this.enabled(row) || byteLength(text) > LIMIT || text.includes('\0')) {
      row.applying = true; replaceText(row.modified, row.editText); row.applying = false;
      this.api.notice('This edit is unavailable or exceeds the 2 MiB preview limit. Use Open in Native Diff.'); return;
    }
    const edit = this.stream(row);
    edit.sequence++;
    const payload = { type: 'edit', repository: edit.repository, id: row.entry.id, stream: edit.stream,
      version: edit.version, revision: edit.revision, sequence: edit.sequence, documentVersion: edit.documentVersion,
      changes: event.changes.map(c => ({ offset: c.rangeOffset, length: c.rangeLength, text: c.text })) };
    if (edit.sequence === 1) payload.before = edit.before;
    row.editText = text;
    this.drafts.set(row.entry.id, { repository: edit.repository, id: row.entry.id, path: row.entry.path, stream: edit.stream,
      before: edit.before, draft: text, sequence: edit.sequence });
    this.persist(); this.update(row);
    this.api.vscode.postMessage(payload);
  }

  save(row) {
    if (!this.enabled(row) || row.edit?.saving) return;
    const edit = this.stream(row); edit.sequence++; edit.saving = true; edit.saveError = undefined;
    this.api.vscode.postMessage({ type: 'save', repository: edit.repository, id: row.entry.id, stream: edit.stream,
      version: edit.version, revision: edit.revision, sequence: edit.sequence, before: edit.before, documentVersion: edit.documentVersion });
    this.update(row);
  }

  apply(row, body) {
    if (this.held(row) || (body.documentVersion && body.documentVersion < (row.edit?.documentVersion || 0))) return false;
    row.applying = true;
    if (row.modified.getValue() !== body.modified) {
      this.end(row); replaceText(row.modified, body.modified);
    }
    row.editText = row.modified.getValue(); row.applying = false;
    return true;
  }

  end(row) {
    if (row.edit && !this.pending(row)) {
      this.api.vscode.postMessage({ type: 'endEdit', repository: this.api.repository, id: row.entry.id, stream: row.edit.stream, sequence: row.edit.sequence + 1 });
      row.edit = undefined;
    }
  }

  release(row) {
    if (this.pending(row)) return false;
    this.end(row); this.rows.delete(row.entry.id); this.renderRecovery();
    return true;
  }

  receive(value) {
    if (value.type === 'editRecovery') {
      for (const done of value.handedOff || []) if (this.drafts.get(done.id)?.stream === done.stream) this.drafts.delete(done.id);
      for (const draft of value.drafts) if (!(value.handedOff || []).some(done => done.id === draft.id && done.stream === draft.stream) && !this.drafts.has(draft.id)) this.drafts.set(draft.id, { ...draft, repository: this.api.repository });
      for (const row of this.rows.values()) this.update(row);
      this.persist(); this.renderRecovery(); return true;
    }
    if (!['editAccepted', 'editRejected', 'editRecovered', 'saveFailed'].includes(value.type)) return false;
    const row = this.rows.get(value.id), edit = row?.edit;
    if (value.type === 'editRecovered') {
      this.drafts.delete(value.id); if (row) { row.edit = undefined; row.renderedRevision = undefined; this.api.changed(row); }
    } else if (edit?.stream === value.stream) {
      if (value.type === 'editAccepted' || value.type === 'saveFailed') {
        edit.ack = Math.max(edit.ack, value.sequence); edit.documentVersion = value.documentVersion;
        if (value.saved || value.type === 'saveFailed') edit.saving = false;
        if (value.type === 'saveFailed') { edit.saveError = value.message; this.api.notice(value.message); }
        if (edit.ack === edit.sequence) { this.drafts.delete(value.id); row.renderedRevision = undefined; this.api.changed(row); }
      } else {
        edit.failed = value.message; edit.saving = false;
        this.api.notice(value.message); this.renderRecovery();
      }
      this.update(row);
      if (value.type === 'editRejected') this.api.changed(row);
    }
    this.persist(); this.renderRecovery(); return true;
  }

  renderRecovery() {
    this.bar.replaceChildren();
    for (const draft of this.drafts.values()) {
      if (this.pending(this.rows.get(draft.id) || {})) continue;
      const button = document.createElement('button'); button.textContent = 'Recover draft: ' + (draft.path || 'changed file');
      button.addEventListener('click', () => this.api.vscode.postMessage({ type: 'recover', repository: this.api.repository,
        id: draft.id, stream: draft.stream, sequence: draft.sequence || 1, draft: draft.draft }));
      this.bar.append(button);
    }
    this.bar.hidden = !this.bar.childElementCount;
  }
}
