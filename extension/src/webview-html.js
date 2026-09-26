// HTML shell for Overseer webviews: design tokens, base styles, codicons and shared scripts, with a
// strict Content-Security-Policy (no remote content, scripts by nonce only).
const vscode = require('vscode');
const os = require('os');
const { randomBytes } = require('crypto');

const SHARED_CSS = ['vendor/codicons/codicon.css', 'tokens.css', 'base.css'];
const SHARED_JS = ['ui.js', 'logos.js'];
const CHAT_JS = ['vendor/marked.umd.js', 'vendor/purify.min.js', 'vendor/highlight.min.js', 'markdown.js', 'conversation.js', 'chat.js'];

function page(webview, extensionUri, { title, css = [], js = [], body = '', bodyAttrs = '', chat = false }) {
  const media = vscode.Uri.joinPath(extensionUri, 'media');
  const nonce = randomBytes(18).toString('base64');
  const asset = name => webview.asWebviewUri(vscode.Uri.joinPath(media, ...name.split('/'))).toString();
  const styles = [...SHARED_CSS, ...(chat ? ['chat.css'] : []), ...css].map(f => `<link rel="stylesheet" href="${asset(f)}">`).join('');
  const scripts = [...SHARED_JS, ...(chat ? CHAT_JS : []), ...js].map(f => `<script nonce="${nonce}" src="${asset(f)}"></script>`).join('');
  const home = JSON.stringify(os.homedir());
  return `<!doctype html><html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}; font-src ${webview.cspSource}; img-src ${webview.cspSource} data:; script-src 'nonce-${nonce}';">
${styles}<title>${title}</title></head><body ${bodyAttrs}>${body}
<script nonce="${nonce}">window.__overseerHome = ${home};</script>${scripts}</body></html>`;
}

module.exports = { page, localRoots: extensionUri => [vscode.Uri.joinPath(extensionUri, 'media')] };
