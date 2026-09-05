(function () {
  if (window.__hmr_initialized) return;
  window.__hmr_initialized = true;
  console.log("[HMR] Initialized");

  // Helper to escape HTML characters
  function escapeHtml(str) {
    if (!str) return '';
    return String(str)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  // Enhanced ERM & JSX Template Syntax Validator
  function validateErmSource(content, filename) {
    if (!content) return null;

    const fnRe = /^\s*export\s+(?:default\s+)?(?:fn|function)\s+[A-Za-z0-9_]+\s*\([^)]*\)\s*\{/m;
    const fnMatch = content.match(fnRe);

    let scanStart = 0;
    let scanEnd = content.length;
    if (fnMatch) {
      scanStart = fnMatch.index + fnMatch[0].length;
      let depth = 1;
      let i = scanStart;
      let escaped = false;
      let inStr = null;
      let inLineComment = false;
      let inBlockComment = false;
      while (i < content.length) {
        const c = content[i];
        if (escaped) { escaped = false; i++; continue; }
        if (c === '\\') { escaped = true; i++; continue; }
        if (inLineComment) {
          if (c === '\n') inLineComment = false;
        } else if (inBlockComment) {
          if (c === '/' && i > 0 && content[i - 1] === '*') inBlockComment = false;
        } else if (inStr) {
          if (c === inStr) inStr = null;
        } else {
          if (c === '/' && i + 1 < content.length && content[i + 1] === '/') {
            inLineComment = true; i++;
          } else if (c === '/' && i + 1 < content.length && content[i + 1] === '*') {
            inBlockComment = true; i++;
          } else if (c === '\'' || c === '"' || c === '`') {
            inStr = c;
          } else if (c === '{') {
            depth++;
          } else if (c === '}') {
            depth--;
            if (depth === 0) {
              scanEnd = i;
              break;
            }
          }
        }
        i++;
      }
    }

    let line = 1;
    let col = 1;
    let tagStack = [];
    let rootCount = 0;
    let inScript = false;
    let i = 0;

    while (i < content.length) {
      if (content[i] === '\n') {
        line++;
        col = 1;
        i++;
        continue;
      }

      // Check for <script>...</script>
      if (!inScript && content.startsWith('<script', i) && (content[i + 7] === '>' || /\s/.test(content[i + 7]))) {
        inScript = true;
        const endScript = content.indexOf('</script>', i);
        if (endScript !== -1) {
          for (let k = i; k < endScript + 9; k++) {
            if (content[k] === '\n') { line++; col = 1; } else { col++; }
          }
          i = endScript + 9;
          inScript = false;
          continue;
        }
      }

      // Check for HTML comments
      if (content.startsWith('<!--', i)) {
        const endComment = content.indexOf('-->', i);
        if (endComment !== -1) {
          for (let k = i; k < endComment + 3; k++) {
            if (content[k] === '\n') { line++; col = 1; } else { col++; }
          }
          i = endComment + 3;
          continue;
        }
      }

      // Check for <style>...</style>
      if (content.startsWith('<style', i) && (content[i + 6] === '>' || /\s/.test(content[i + 6]))) {
        const endStyle = content.indexOf('</style>', i);
        if (endStyle !== -1) {
          for (let k = i; k < endStyle + 8; k++) {
            if (content[k] === '\n') { line++; col = 1; } else { col++; }
          }
          i = endStyle + 8;
          continue;
        }
      }

      // If function template, only check JSX in markup section
      if (fnMatch && (i < scanStart || i >= scanEnd)) {
        col++;
        i++;
        continue;
      }

      // Look for JSX tag opening
      if (content[i] === '<') {
        const tagStartLine = line;
        const tagStartCol = col;
        const tagStartPos = i;

        // Closing tag: </...
        if (content[i + 1] === '/') {
          // Fragment closing: </>
          if (content[i + 2] === '>') {
            const closingCol = tagStartCol + 2; // Position of '>'
            if (tagStack.length === 0) {
              return {
                title: "Expression expected",
                message: "Expression expected",
                description: "Parsing ecmascript source code failed",
                line: tagStartLine,
                col: closingCol,
                pos: tagStartPos
              };
            }
            const top = tagStack.pop();
            if (top.tag !== '') {
              // Mismatched closing tag! Expected tag closing, encountered </>
              return {
                title: "Expression expected",
                message: "Expression expected",
                description: "Parsing ecmascript source code failed",
                line: tagStartLine,
                col: closingCol,
                pos: tagStartPos
              };
            }
            col += 3;
            i += 3;
            continue;
          } else {
            // Named closing tag: </tag_name>
            let tagCloseEnd = content.indexOf('>', i + 2);
            if (tagCloseEnd === -1) {
              return {
                title: "Expected '>', got '<eof>'",
                message: "Expected '>', got '<eof>'",
                description: "Parsing ecmascript source code failed",
                line: tagStartLine,
                col: tagStartCol,
                pos: tagStartPos
              };
            }
            const closeTagName = content.slice(i + 2, tagCloseEnd).trim();
            if (tagStack.length === 0) {
              return {
                title: `Unexpected closing tag </${closeTagName}>`,
                message: `Unexpected closing tag </${closeTagName}>`,
                description: "Parsing ecmascript source code failed",
                line: tagStartLine,
                col: tagStartCol,
                pos: tagStartPos
              };
            }
            const top = tagStack.pop();
            if (top.tag === '') {
              return {
                title: `Expected '</>', got '</${closeTagName}>'`,
                message: `Expected '</>', got '</${closeTagName}>'`,
                description: "Parsing ecmascript source code failed",
                line: tagStartLine,
                col: tagStartCol,
                pos: tagStartPos
              };
            }
            if (top.tag !== closeTagName) {
              return {
                title: `Expected '</${top.tag}>', got '</${closeTagName}>'`,
                message: `Expected '</${top.tag}>', got '</${closeTagName}>'`,
                description: "Parsing ecmascript source code failed",
                line: tagStartLine,
                col: tagStartCol,
                pos: tagStartPos
              };
            }
            for (let k = i; k <= tagCloseEnd; k++) {
              if (content[k] === '\n') { line++; col = 1; } else { col++; }
            }
            i = tagCloseEnd + 1;
            continue;
          }
        }

        // Fragment opening: <>
        if (content[i + 1] === '>') {
          if (fnMatch && tagStack.length === 0) {
            rootCount++;
            if (rootCount > 1) {
              return {
                title: "Adjacent JSX elements must be wrapped in an enclosing tag",
                message: "Adjacent JSX elements must be wrapped in an enclosing tag. Did you want a JSX fragment <>...</>?",
                description: "Parsing ecmascript source code failed",
                line: tagStartLine,
                col: tagStartCol,
                pos: tagStartPos
              };
            }
          }
          tagStack.push({ tag: '', line: tagStartLine, col: tagStartCol });
          col += 2;
          i += 2;
          continue;
        }

        // Standard tag opening: <tag_name ...>
        const nextChar = content[i + 1];
        if (nextChar && (/[a-zA-Z]/.test(nextChar) || nextChar === '_')) {
          let tagEnd = -1;
          let braceDepth = 0;
          let quoteChar = null;
          for (let k = i + 1; k < content.length; k++) {
            const ch = content[k];
            if (quoteChar) {
              if (ch === quoteChar && content[k - 1] !== '\\') quoteChar = null;
            } else if (ch === '"' || ch === '\'' || ch === '`') {
              quoteChar = ch;
            } else if (ch === '{') {
              braceDepth++;
            } else if (ch === '}') {
              if (braceDepth > 0) braceDepth--;
            } else if (ch === '>' && braceDepth === 0) {
              tagEnd = k;
              break;
            }
          }

          if (tagEnd === -1) {
            return {
              title: "Expected '>', got '<eof>'",
              message: "Expected '>', got '<eof>'",
              description: "Parsing ecmascript source code failed",
              line: tagStartLine,
              col: tagStartCol,
              pos: tagStartPos
            };
          }

          const tagInner = content.slice(i + 1, tagEnd).trim();
          const matchTagName = tagInner.match(/^([a-zA-Z0-9_\-\.]+)/);
          const tagName = matchTagName ? matchTagName[1] : '';
          const isCustomComponent = tagName.length > 0 && tagName[0] >= 'A' && tagName[0] <= 'Z';
          const isSelfClosing = tagInner.endsWith('/') || (!isCustomComponent && ['img', 'input', 'br', 'hr', 'meta', 'link'].includes(tagName.toLowerCase()));

          if (fnMatch && tagStack.length === 0) {
            rootCount++;
            if (rootCount > 1) {
              return {
                title: "Adjacent JSX elements must be wrapped in an enclosing tag",
                message: "Adjacent JSX elements must be wrapped in an enclosing tag. Did you want a JSX fragment <>...</>?",
                description: "Parsing ecmascript source code failed",
                line: tagStartLine,
                col: tagStartCol,
                pos: tagStartPos
              };
            }
          }

          if (!isSelfClosing) {
            tagStack.push({ tag: tagName, line: tagStartLine, col: tagStartCol });
          }

          for (let k = i; k <= tagEnd; k++) {
            if (content[k] === '\n') { line++; col = 1; } else { col++; }
          }
          i = tagEnd + 1;
          continue;
        }
      }

      col++;
      i++;
    }

    if (tagStack.length > 0) {
      const top = tagStack[tagStack.length - 1];
      return {
        title: top.tag ? `Unclosed <${top.tag}> tag` : "Expected '>', got '<eof>'",
        message: top.tag ? `Unclosed <${top.tag}> tag` : "Expected '>', got '<eof>'",
        description: "Parsing ecmascript source code failed",
        line: top.line,
        col: top.col,
        pos: top.pos
      };
    }

    return null;
  }

  // Next.js Codeframe Generator with Line Highlight and Column Caret Pointer
  function buildCodeFrame(source, line, col, errorTitle) {
    if (!source || !line) return '';
    const lines = source.split('\n');
    const startLine = Math.max(1, line - 2);
    const endLine = Math.min(lines.length, line + 3);
    const maxLineNumWidth = String(endLine).length;

    let out = '';
    if (errorTitle) {
      out += `<div class="nextjs-codeframe-headline">Error: ${escapeHtml(errorTitle)}</div>`;
    }

    for (let l = startLine; l <= endLine; l++) {
      const lineText = lines[l - 1] || '';
      const isErr = (l === line);
      const paddedNum = String(l).padStart(maxLineNumWidth, ' ');

      if (isErr) {
        out += `<div class="nextjs-codeframe-line nextjs-line-errored">` +
          `<span class="nextjs-line-marker">&gt;</span> ` +
          `<span class="nextjs-line-num">${paddedNum} |</span> ` +
          `<span class="nextjs-line-code">${escapeHtml(lineText)}</span>` +
          `</div>`;

        if (col && col > 0) {
          const caretIndent = Math.max(0, col - 1);
          const pointerSpaces = ' '.repeat(caretIndent);
          const gutterSpaces = ' '.repeat(maxLineNumWidth);
          out += `<div class="nextjs-codeframe-line nextjs-line-caret">` +
            `<span class="nextjs-line-marker"> </span> ` +
            `<span class="nextjs-line-num">${gutterSpaces} |</span> ` +
            `<span class="nextjs-caret-pointer">${pointerSpaces}^</span>` +
            `</div>`;
        }
      } else {
        out += `<div class="nextjs-codeframe-line">` +
          `<span class="nextjs-line-marker"> </span> ` +
          `<span class="nextjs-line-num">${paddedNum} |</span> ` +
          `<span class="nextjs-line-code">${escapeHtml(lineText)}</span>` +
          `</div>`;
      }
    }
    return out;
  }

  // Next.js Dev Overlay CSS Styles
  const OVERLAY_CSS = `
#erm-error-overlay {
  position: fixed;
  top: 0;
  right: 0;
  bottom: 0;
  left: 0;
  z-index: 2147483647;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding: 10vh 16px 24px;
  background: rgba(0, 0, 0, 0.75);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  color: #ededed;
  overflow-y: auto;
  box-sizing: border-box;
  animation: nextjs-fade-in 0.15s cubic-bezier(0.16, 1, 0.3, 1);
}

@keyframes nextjs-fade-in {
  from { opacity: 0; }
  to { opacity: 1; }
}

.nextjs-dialog-card {
  background: #0a0a0a;
  border: 1px solid rgba(255, 255, 255, 0.14);
  border-radius: 16px;
  width: 100%;
  max-width: 860px;
  box-shadow: 0 24px 64px rgba(0, 0, 0, 0.85), 0 0 0 1px rgba(255, 255, 255, 0.05);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  position: relative;
  animation: nextjs-scale-in 0.2s cubic-bezier(0.16, 1, 0.3, 1);
  box-sizing: border-box;
}

@keyframes nextjs-scale-in {
  from { transform: scale(0.97); opacity: 0; }
  to { transform: scale(1); opacity: 1; }
}

.nextjs-dialog-header {
  padding: 20px 24px 16px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  display: flex;
  flex-direction: column;
  background: #0a0a0a;
}

.nextjs-header-top-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}

.nextjs-error-type-badge {
  background: #2a1314;
  color: #ff6369;
  border: 1px solid rgba(255, 99, 105, 0.25);
  font-size: 12px;
  font-weight: 600;
  font-family: "Geist Mono", "SFMono-Regular", Menlo, Monaco, Consolas, monospace;
  padding: 2px 8px;
  border-radius: 6px;
  letter-spacing: 0.02em;
  display: inline-flex;
  align-items: center;
}

.nextjs-error-type-badge.badge-runtime {
  background: #2a1314;
  color: #ff6369;
}

.nextjs-error-type-badge.badge-warning {
  background: #271700;
  color: #f1a10d;
  border-color: rgba(241, 161, 13, 0.3);
}

.nextjs-header-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.nextjs-close-btn {
  width: 28px;
  height: 28px;
  border-radius: 6px;
  border: none;
  background: transparent;
  color: #8f8f8f;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.15s ease;
  padding: 0;
}

.nextjs-close-btn:hover {
  background: rgba(255, 255, 255, 0.1);
  color: #ffffff;
}

.nextjs-error-title {
  margin: 0 0 6px 0;
  font-size: 19px;
  font-weight: 600;
  line-height: 1.35;
  color: #ededed;
  letter-spacing: -0.02em;
  word-break: break-word;
}

.nextjs-error-location {
  font-size: 13px;
  font-family: "Geist Mono", "SFMono-Regular", Menlo, Monaco, Consolas, monospace;
  color: #a0a0a0;
  margin: 0;
}

.nextjs-dialog-body {
  padding: 20px 24px;
  overflow-y: auto;
  max-height: calc(85vh - 180px);
  box-sizing: border-box;
}

.nextjs-codeframe {
  background: #000000;
  border: 1px solid #2e2e2e;
  border-radius: 12px;
  overflow: hidden;
  font-family: "Geist Mono", "SFMono-Regular", Menlo, Monaco, Consolas, monospace;
  margin: 0 0 16px 0;
}

.nextjs-codeframe-header {
  background: #121214;
  border-bottom: 1px solid #242426;
  padding: 8px 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 12px;
  color: #8f8f8f;
}

.nextjs-codeframe-link {
  display: flex;
  align-items: center;
  gap: 8px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.nextjs-codeframe-link svg {
  color: #71717a;
  flex-shrink: 0;
}

.nextjs-copy-btn {
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.1);
  color: #a0a0a0;
  padding: 4px 8px;
  border-radius: 6px;
  font-size: 11px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 4px;
  transition: all 0.15s ease;
}

.nextjs-copy-btn:hover {
  background: rgba(255, 255, 255, 0.1);
  color: #ffffff;
}

.nextjs-codeframe-pre {
  margin: 0;
  padding: 14px;
  background: #000000;
  overflow-x: auto;
  font-size: 13px;
  line-height: 20px;
  color: #ededed;
}

.nextjs-codeframe-headline {
  color: #ff6369;
  font-weight: 600;
  margin-bottom: 8px;
}

.nextjs-codeframe-line {
  display: flex;
  align-items: center;
  white-space: pre;
}

.nextjs-line-marker {
  width: 14px;
  color: #ff6369;
  font-weight: bold;
  user-select: none;
  flex-shrink: 0;
}

.nextjs-line-num {
  color: #636366;
  user-select: none;
  margin-right: 8px;
  flex-shrink: 0;
}

.nextjs-line-code {
  color: #ededed;
}

.nextjs-line-errored {
  background: rgba(229, 72, 77, 0.16);
  box-shadow: inset 3px 0 0 #e5484d;
  margin-left: -14px;
  margin-right: -14px;
  padding-left: 14px;
  padding-right: 14px;
}

.nextjs-line-errored .nextjs-line-code {
  color: #ff8589;
  font-weight: 500;
}

.nextjs-caret-pointer {
  color: #ff6369;
  font-weight: bold;
}

.nextjs-error-description {
  color: #a0a0a0;
  font-size: 13.5px;
  line-height: 1.5;
  margin: 14px 0 0;
  word-break: break-word;
}

.nextjs-stack-toggle {
  margin-top: 16px;
}

.nextjs-stack-toggle summary {
  font-size: 12px;
  color: #8f8f8f;
  font-family: "Geist Mono", monospace;
  cursor: pointer;
  user-select: none;
  margin-bottom: 8px;
}

.nextjs-stack-toggle summary:hover {
  color: #ededed;
}

.nextjs-stack-pre {
  background: #000000;
  border: 1px solid #242426;
  border-radius: 8px;
  padding: 12px;
  font-size: 12px;
  line-height: 1.6;
  color: #8f8f8f;
  font-family: "Geist Mono", monospace;
  overflow-x: auto;
  max-height: 200px;
  margin: 0;
}

.nextjs-dialog-footer {
  border-top: 1px solid rgba(255, 255, 255, 0.08);
  padding: 12px 24px;
  font-size: 12px;
  color: #71717a;
  background: #0d0d0f;
  display: flex;
  align-items: center;
  gap: 8px;
}

.nextjs-footer-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #ef4444;
  flex-shrink: 0;
}
  `;

  // Main Next.js Dev Overlay Renderer
  window.__erm_show_error_overlay = function (err) {
    let overlay = document.getElementById('erm-error-overlay');
    if (overlay) overlay.remove();

    let style = document.getElementById('erm-error-overlay-styles');
    if (!style) {
      style = document.createElement('style');
      style.id = 'erm-error-overlay-styles';
      style.textContent = OVERLAY_CSS;
      (document.head || document.documentElement).appendChild(style);
    }

    overlay = document.createElement('div');
    overlay.id = 'erm-error-overlay';

    const card = document.createElement('div');
    card.className = 'nextjs-dialog-card';

    const errorType = err.type || 'Build Error';
    let file = err.file || window.__erm_filename || 'unknown';
    let line = err.line;
    let col = err.col;

    // Parse coordinates from file string if present (e.g. "path/file.erm:5:17")
    const coordMatch = file.match(/:(\d+)(?::(\d+))?$/);
    if (coordMatch) {
      if (!line) line = parseInt(coordMatch[1], 10);
      if (!col && coordMatch[2]) col = parseInt(coordMatch[2], 10);
      file = file.replace(/:(\d+)(?::(\d+))?$/, '');
    }

    let title = err.title || 'Expression expected';
    title = title.replace(/^Error:\s*/, '');
    const description = err.description || 'Parsing ecmascript source code failed';

    // Format location display e.g. "./app/page.tsx (5:17)"
    const displayFilePath = file.startsWith('/') || file.startsWith('./') ? file : `./${file}`;
    const locationStr = line ? `${displayFilePath} (${line}${col ? `:${col}` : ''})` : displayFilePath;

    // Header
    const header = document.createElement('div');
    header.className = 'nextjs-dialog-header';
    header.innerHTML = `
      <div class="nextjs-header-top-row">
        <span class="nextjs-error-type-badge ${errorType.toLowerCase().includes('runtime') ? 'badge-runtime' : ''}">${escapeHtml(errorType)}</span>
        <div class="nextjs-header-actions">
          <button class="nextjs-close-btn" aria-label="Close" title="Close (Esc)">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <line x1="18" y1="6" x2="6" y2="18"></line>
              <line x1="6" y1="6" x2="18" y2="18"></line>
            </svg>
          </button>
        </div>
      </div>
      <h1 class="nextjs-error-title">${escapeHtml(title)}</h1>
      <div class="nextjs-error-location">${escapeHtml(locationStr)}</div>
    `;

    // Body
    const body = document.createElement('div');
    body.className = 'nextjs-dialog-body';

    const codeFrameContainer = document.createElement('div');
    codeFrameContainer.className = 'nextjs-codeframe';
    codeFrameContainer.innerHTML = `
      <div class="nextjs-codeframe-header">
        <div class="nextjs-codeframe-link">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path>
            <polyline points="14 2 14 8 20 8"></polyline>
          </svg>
          <span class="nextjs-codeframe-path">${escapeHtml(displayFilePath)}${line ? `:${line}${col ? `:${col}` : ''}` : ''}</span>
        </div>
        <button class="nextjs-copy-btn" title="Copy error">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
          </svg>
          <span class="copy-text">Copy</span>
        </button>
      </div>
      <pre class="nextjs-codeframe-pre"><code class="nextjs-codeframe-content">${buildCodeFrame(err.source, line, col, title)}</code></pre>
    `;

    body.appendChild(codeFrameContainer);

    const descEl = document.createElement('div');
    descEl.className = 'nextjs-error-description';
    descEl.textContent = description;
    body.appendChild(descEl);

    if (err.stack) {
      const stackToggle = document.createElement('details');
      stackToggle.className = 'nextjs-stack-toggle';
      stackToggle.innerHTML = `
        <summary>Call Stack</summary>
        <pre class="nextjs-stack-pre">${escapeHtml(err.stack)}</pre>
      `;
      body.appendChild(stackToggle);
    }

    // Footer
    const footer = document.createElement('div');
    footer.className = 'nextjs-dialog-footer';
    footer.innerHTML = `
      <span class="nextjs-footer-dot"></span>
      <span>Fix the issue in your code to automatically reload.</span>
    `;

    card.appendChild(header);
    card.appendChild(body);
    card.appendChild(footer);
    overlay.appendChild(card);

    // Event Listeners
    const closeOverlay = () => {
      overlay.remove();
      document.removeEventListener('keydown', escHandler);
    };

    const escHandler = (e) => {
      if (e.key === 'Escape') {
        closeOverlay();
      }
    };
    document.addEventListener('keydown', escHandler);

    const closeBtn = card.querySelector('.nextjs-close-btn');
    if (closeBtn) closeBtn.onclick = closeOverlay;

    // Copy error action
    const copyBtn = card.querySelector('.nextjs-copy-btn');
    if (copyBtn) {
      copyBtn.onclick = () => {
        const fullErrorText = `${errorType}\n\n${title}\n${locationStr}\n\nError: ${title}\n` +
          (err.source && line ? buildCodeFrame(err.source, line, col, '').replace(/<[^>]+>/g, '') + '\n' : '') +
          `${description}`;
        navigator.clipboard.writeText(fullErrorText).then(() => {
          const copyLabel = copyBtn.querySelector('.copy-text');
          if (copyLabel) copyLabel.textContent = 'Copied!';
          setTimeout(() => {
            if (copyLabel) copyLabel.textContent = 'Copy';
          }, 1500);
        });
      };
    }

    (document.body || document.documentElement).appendChild(overlay);

    // If source wasn't passed directly, fetch it dynamically from /__erm_src/
    if (!err.source && file) {
      fetch('/__erm_src/' + file.replace(/^\.?\//, ''))
        .then(r => r.ok ? r.text() : '')
        .then(src => {
          if (!src) return;
          err.source = src;
          // If line wasn't known, try running the validator
          if (!line) {
            const detected = validateErmSource(src, file);
            if (detected) {
              line = detected.line;
              col = detected.col;
              title = detected.title;
            }
          }
          const locEl = card.querySelector('.nextjs-error-location');
          if (locEl && line) {
            locEl.textContent = `${displayFilePath} (${line}${col ? `:${col}` : ''})`;
          }
          const pathEl = card.querySelector('.nextjs-codeframe-path');
          if (pathEl && line) {
            pathEl.textContent = `${displayFilePath}:${line}${col ? `:${col}` : ''}`;
          }
          const codeEl = card.querySelector('.nextjs-codeframe-content');
          if (codeEl) {
            codeEl.innerHTML = buildCodeFrame(src, line, col, title);
          }
        })
        .catch(e => console.error("[HMR] Error resolving source for code frame:", e));
    }
  };

  // Source Validation and HMR Execution
  function validateSourceAndRun(next) {
    if (!window.__erm_filename) {
      next(false);
      return;
    }
    fetch('/__erm_src/' + window.__erm_filename)
      .then(r => r.ok ? r.text() : '')
      .then(src => {
        const error = validateErmSource(src, window.__erm_filename);
        if (error) {
          if (typeof window.__erm_show_error_overlay === 'function') {
            window.__erm_show_error_overlay({
              type: 'Build Error',
              file: window.__erm_filename,
              title: error.title || 'Expression expected',
              message: error.message || error.title,
              description: error.description || 'Parsing ecmascript source code failed',
              line: error.line,
              col: error.col,
              source: src
            });
          }
        } else {
          const overlay = document.getElementById('erm-error-overlay');
          const hasOverlay = !!overlay;
          if (overlay) overlay.remove();
          const styleOverlay = document.getElementById('erm-error-overlay-styles');
          if (styleOverlay) styleOverlay.remove();
          next(hasOverlay);
        }
      })
      .catch(err => {
        console.error("Failed to fetch source for validation:", err);
        next(false);
      });
  }

  if (window.__erm_filename) {
    validateSourceAndRun(() => { });
  }

  window.__hmr_hooks = window.__hmr_hooks || { dispose: [], accept: [] };
  window.hmr = {
    data: window.__hmr_data || {},
    accept: (cb) => window.__hmr_hooks.accept.push(cb),
    dispose: (cb) => window.__hmr_hooks.dispose.push(cb),
    invalidate: () => location.reload()
  };
  window.__hmr_data = window.hmr.data;

  window.__hmr_intervals = window.__hmr_intervals || [];
  const originalSetInterval = window.setInterval;
  window.setInterval = function (fn, t) {
    let id = originalSetInterval(fn, t);
    window.__hmr_intervals.push(id);
    return id;
  };

  window.__hmr_listeners = window.__hmr_listeners || [];
  const originalDocAddEventListener = document.addEventListener;
  document.addEventListener = function (type, listener, options) {
    window.__hmr_listeners.push({ target: document, type, listener, options });
    return originalDocAddEventListener.call(document, type, listener, options);
  };

  const originalWinAddEventListener = window.addEventListener;
  window.addEventListener = function (type, listener, options) {
    window.__hmr_listeners.push({ target: window, type, listener, options });
    return originalWinAddEventListener.call(window, type, listener, options);
  };

  const originalElementAddEventListener = Element.prototype.addEventListener;
  Element.prototype.addEventListener = function (type, listener, options) {
    window.__hmr_listeners.push({ target: this, type, listener, options });
    return originalElementAddEventListener.call(this, type, listener, options);
  };

  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const ws = new WebSocket(protocol + '//' + location.host + '/__hmr');
  ws.onmessage = (e) => {
    const data = JSON.parse(e.data);
    if (data.type === 'reload') {
      location.reload();
    } else if (data.type === 'update') {
      const path = data.path || 'unknown';
      console.log("[HMR] Update received for: " + path);

      if (path.endsWith('.css')) {
        let links = document.querySelectorAll('link[rel="stylesheet"]');
        let found = false;
        links.forEach(link => {
          if (link.href.includes(path)) {
            link.href = path + '?t=' + new Date().getTime();
            found = true;
          }
        });
        if (found) return;
      }

      validateSourceAndRun((wasError) => {
        fetch(location.href)
          .then(r => {
            if (!r.ok || r.status === 500) {
              return r.text().then(text => {
                const parser = new DOMParser();
                const doc = parser.parseFromString(text, 'text/html');
                const bodyEl = doc.body || doc.documentElement;
                const file = bodyEl.getAttribute('data-compile-error-file');
                const message = bodyEl.getAttribute('data-compile-error-message');
                if (file && message) {
                  if (typeof window.__erm_show_error_overlay === 'function') {
                    window.__erm_show_error_overlay({
                      type: 'Build Error',
                      file: file,
                      title: message.replace(/^Error:\s*/, ''),
                      message: message,
                      description: 'Parsing ecmascript source code failed'
                    });
                  }
                } else {
                  console.error("Server compilation failed:", text);
                }
                throw new Error("Compilation error overlay shown");
              });
            }
            return r.text();
          })
          .then(html => {
            if (wasError || !document.querySelector('script.__erm_script') || (document.body.firstElementChild && document.body.firstElementChild.id === 'erm-error-overlay')) {
              location.reload();
              return;
            }

            const parser = new DOMParser();
            const doc = parser.parseFromString(html, 'text/html');
            document.title = doc.title;

            function morph(oldNode, newNode) {
              if (oldNode.nodeType !== newNode.nodeType || oldNode.tagName !== newNode.tagName) {
                oldNode.replaceWith(newNode.cloneNode(true));
                return;
              }
              if (oldNode.nodeType === Node.TEXT_NODE) {
                if (oldNode.textContent !== newNode.textContent) oldNode.textContent = newNode.textContent;
                return;
              }
              const oldAttrs = oldNode.attributes;
              const newAttrs = newNode.attributes;
              if (oldAttrs && newAttrs) {
                for (let i = 0; i < newAttrs.length; i++) {
                  const attr = newAttrs[i];
                  if (oldNode.getAttribute(attr.name) !== attr.value) oldNode.setAttribute(attr.name, attr.value);
                }
                for (let i = 0; i < oldAttrs.length; i++) {
                  const attr = oldAttrs[i];
                  if (!newNode.hasAttribute(attr.name)) oldNode.removeAttribute(attr.name);
                }
              }
              const oldChildren = Array.from(oldNode.childNodes);
              const newChildren = Array.from(newNode.childNodes);
              const max = Math.max(oldChildren.length, newChildren.length);
              for (let i = 0; i < max; i++) {
                if (i >= oldChildren.length) {
                  oldNode.appendChild(newChildren[i].cloneNode(true));
                } else if (i >= newChildren.length) {
                  oldNode.removeChild(oldChildren[i]);
                } else {
                  morph(oldChildren[i], newChildren[i]);
                }
              }
            }

            const newStyles = doc.querySelectorAll('style');
            if (newStyles.length > 0) {
              let styleContainer = document.getElementById('__erm_styles');
              if (!styleContainer) {
                styleContainer = document.createElement('div');
                styleContainer.id = '__erm_styles';
                document.head.appendChild(styleContainer);
              }
              styleContainer.innerHTML = '';
              newStyles.forEach(s => styleContainer.appendChild(s.cloneNode(true)));
            }

            window.__hmr_hooks.dispose.forEach(cb => { try { cb(window.hmr.data); } catch (err) { } });
            window.__hmr_hooks.dispose = [];

            const oldAccepts = window.__hmr_hooks.accept;
            window.__hmr_hooks.accept = [];

            window.__hmr_intervals.forEach(clearInterval);
            window.__hmr_intervals = [];
            window.__hmr_listeners.forEach(({ target, type, listener, options }) => {
              target.removeEventListener(type, listener, options);
              if (target.__erm_listener_added) delete target.__erm_listener_added;
            });
            window.__hmr_listeners = [];

            // Clean up old erm scripts
            document.querySelectorAll('script.__erm_script').forEach(s => s.remove());

            morph(document.body, doc.body);

            const newScripts = doc.querySelectorAll('script');
            newScripts.forEach(s => {
              if (s.textContent.includes("__hmr_initialized")) return;
              const newScript = document.createElement('script');
              newScript.text = s.innerHTML;
              if (s.className) newScript.className = s.className;
              const sType = s.getAttribute('type');
              if (sType) newScript.type = sType;
              try {
                if (s.src) {
                  let sUrl = new URL(s.src, location.href);
                  sUrl.searchParams.set('t', new Date().getTime());
                  newScript.src = sUrl.href;
                  document.head.appendChild(newScript);
                } else if (sType === 'module') {
                  document.head.appendChild(newScript);
                } else {
                  (0, eval)(s.innerHTML);
                }
              } catch (err) {
                console.error("[HMR] Script evaluation failed:", err);
                if (typeof window.__erm_show_error_overlay === 'function') {
                  let file = window.__erm_filename || location.pathname;
                  let stack = err.stack || '';
                  let origLine = null;
                  const match = stack.match(/(?::|\()(\d+)(?::(\d+))?\)?$/m) || stack.match(/(?::|\()(\d+)(?::(\d+))?\)?$/);
                  if (match) {
                    const inlineLine = parseInt(match[1], 10);
                    const inlineCol = match[2] ? parseInt(match[2], 10) : 0;
                    const scriptLines = s.innerHTML.split('\n');
                    for (let idx = inlineLine - 1; idx >= 0; idx--) {
                      const lineContent = scriptLines[idx] || '';
                      const commentMatch = lineContent.match(/\/\/\s*line:(\d+)\s*$/);
                      if (commentMatch) {
                        origLine = parseInt(commentMatch[1], 10);
                        break;
                      }
                    }
                    if (origLine !== null) {
                      file = `${file}:${origLine}${inlineCol ? ':' + inlineCol : ''}`;
                    }
                  }
                  window.__erm_show_error_overlay({
                    type: 'Runtime Error',
                    file: file,
                    title: err.name || 'SyntaxError',
                    message: err.message || 'Invalid or unexpected token',
                    description: 'Runtime execution error in component script',
                    stack: stack,
                    line: origLine
                  });
                }
              }
            });
            document.dispatchEvent(new Event('DOMContentLoaded'));
            window.dispatchEvent(new Event('load'));

            oldAccepts.forEach(cb => { try { cb(); } catch (err) { } });

            if (window.__erm_update) window.__erm_update();
          })
          .catch(err => {
            if (err.message !== "Compilation error overlay shown") {
              console.error("[HMR] Update failed:", err);
            }
          });
      });
    }
  };

  const checkInitialCompileError = () => {
    const bodyEl = document.body || document.documentElement;
    if (bodyEl) {
      const file = bodyEl.getAttribute('data-compile-error-file');
      const message = bodyEl.getAttribute('data-compile-error-message');
      if (file && message) {
        window.__erm_show_error_overlay({
          type: 'Build Error',
          file: file,
          title: message.replace(/^Error:\s*/, ''),
          message: message,
          description: 'Parsing ecmascript source code failed'
        });
      }
    }
  };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', checkInitialCompileError);
  } else {
    checkInitialCompileError();
  }
})();
