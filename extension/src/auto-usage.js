// Local Auto learning is inspected, exported, and cleared through the daemon.
// Only allowlisted, content-free fields reach the native picker or report.
const vscode = require('vscode');

const count = value => Number.isSafeInteger(value) && value >= 0 ? value : undefined;
const text = value => typeof value === 'string' && value.length <= 120 ? value : undefined;

function summary(raw) {
  const input = count(raw?.usage?.input_tokens ?? raw?.input);
  const output = count(raw?.usage?.output_tokens ?? raw?.output);
  const total = input !== undefined && output !== undefined && Number.isSafeInteger(input + output)
    ? input + output : undefined;
  return {
    model: text(raw?.model) || 'Unknown model', effort: text(raw?.effort) || 'effort unknown',
    status: text(raw?.status) || 'status unknown',
    date: Number.isSafeInteger(raw?.ended_ms) && raw.ended_ms > 0 ? new Date(raw.ended_ms).toLocaleString() : text(raw?.date) || 'time unknown',
    total, input, output,
  };
}

function rows(records, paused) {
  const items = (Array.isArray(records) ? records : []).slice(0, 30).map(raw => {
    const record = summary(raw);
    return { action: 'record', record, label: `${record.model} · ${record.effort}`,
      description: `${record.status} · ${record.date}`,
      detail: `${record.total === undefined ? 'Token activity unavailable' : `${record.total} reported tokens`} · subscription draw unverified` };
  });
  if (!items.length) items.push({ action: 'empty', label: paused ? 'Local learning is paused' : 'No Auto work recorded yet',
    detail: paused ? 'Execution can continue; usage history is temporarily unavailable.' : 'Completed Auto work appears here when reported.' });
  return items;
}

function report(raw) {
  const record = summary(raw);
  return `# Auto work usage\n\n${record.model} · ${record.effort}\n\n` +
    `Status: ${record.status}\n\nFinished: ${record.date}\n\n` +
    (record.total === undefined ? 'Reported token activity: unavailable\n\n' :
      `Reported token activity: ${record.total} tokens (${record.input} input, ${record.output} output)\n\n`) +
    'Subscription allowance draw: unverified. Reported token activity is not a subscription charge.\n';
}

class AutoUsage {
  constructor(client, context) {
    this.client = client;
    this.reports = new Map();
    this.serial = 0;
    if (context) context.subscriptions.push(
      vscode.workspace.registerTextDocumentContentProvider('overseer-auto-usage', this),
      vscode.workspace.onDidCloseTextDocument(document => {
        if (document.uri.scheme === 'overseer-auto-usage') this.reports.delete(document.uri.path);
      }));
  }

  provideTextDocumentContent(uri) {
    return this.reports.get(uri.path) || 'This local usage report is no longer available. Reopen Auto usage.';
  }

  async show() {
    const state = await this.client.request('auto.usage.work.list', { limit: 30 });
    const items = [
      ...rows(state.work_units, state.learning_paused),
      { action: 'export', label: 'Export local usage…', detail: 'Save a redacted JSON file on this Mac; nothing is uploaded.' },
      { action: 'clear', label: 'Clear local usage…', detail: 'Remove learning records and summaries; agents and their history stay.' },
    ];
    const picked = await vscode.window.showQuickPick(items, { title: 'Auto usage',
      placeHolder: state.learning_paused ? 'Local learning is paused; agents can keep working.' : 'Reported activity is not subscription allowance.',
      matchOnDescription: true, matchOnDetail: true });
    if (!picked) return;
    if (picked.action === 'record') {
      const uri = vscode.Uri.parse(`overseer-auto-usage:/work/auto-work-usage-${++this.serial}.md`);
      if (this.reports.size >= 100) this.reports.delete(this.reports.keys().next().value);
      this.reports.set(uri.path, report(picked.record));
      const document = await vscode.workspace.openTextDocument(uri);
      await vscode.window.showTextDocument(document, { preview: true });
    } else if (picked.action === 'export') await this.export();
    else if (picked.action === 'clear') await this.clear();
  }

  async export() {
    const uri = await vscode.window.showSaveDialog({ title: 'Export local Auto usage', saveLabel: 'Export',
      filters: { 'JSON files': ['json'] } });
    if (!uri) return;
    if (uri.scheme !== 'file' || !uri.fsPath) {
      await vscode.window.showInformationMessage('Choose a file on this Mac for the local Auto usage export.');
      return;
    }
    const result = await this.client.request('auto.usage.export', { path: uri.fsPath });
    await vscode.window.showInformationMessage(`Exported ${result.count} local usage record${result.count === 1 ? '' : 's'}.`);
  }

  async clear() {
    const action = 'Clear local Auto usage';
    const answer = await vscode.window.showWarningMessage(
      'Clear local Auto usage and learning? Agents and their history stay available.', { modal: true }, action);
    if (answer !== action) return;
    const result = await this.client.request('auto.usage.clear', {});
    await vscode.window.showInformationMessage(`Cleared ${result.deleted} local usage record${result.deleted === 1 ? '' : 's'}.`);
  }
}

module.exports = { AutoUsage, rows, report };
