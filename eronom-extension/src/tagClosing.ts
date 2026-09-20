// eronom-extension/src/tagClosing.ts - Automatic Tag Closing for ERM & EDS
import * as vscode from 'vscode';

const VOID_ELEMENTS = new Set([
  'area',
  'base',
  'br',
  'col',
  'embed',
  'hr',
  'img',
  'input',
  'link',
  'meta',
  'param',
  'source',
  'track',
  'wbr',
]);

/**
 * Checks if quotes or braces are unclosed inside tag text (to avoid matching '>' inside attributes).
 */
export function isInsideQuotesOrBraces(text: string): boolean {
  let inDouble = false;
  let inSingle = false;
  let inBraces = 0;

  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === '"' && !inSingle) {
      inDouble = !inDouble;
    } else if (ch === "'" && !inDouble) {
      inSingle = !inSingle;
    } else if (ch === '{' && !inSingle && !inDouble) {
      inBraces++;
    } else if (ch === '}' && !inSingle && !inDouble && inBraces > 0) {
      inBraces--;
    }
  }

  return inDouble || inSingle || inBraces > 0;
}

/**
 * Extracts the opening tag name from text preceding a typed '>'.
 */
export function getAutoClosingTag(textBeforeGt: string): string | null {
  const lastOpenAngle = textBeforeGt.lastIndexOf('<');
  if (lastOpenAngle === -1) {
    return null;
  }

  const tagChunk = textBeforeGt.substring(lastOpenAngle);

  // 1. Ignore closing tags: '</...'
  if (tagChunk.startsWith('</')) {
    return null;
  }

  // 2. Ignore comments or doctypes: '<!--...', '<!...'
  if (tagChunk.startsWith('<!--') || tagChunk.startsWith('<!') || tagChunk.startsWith('<?')) {
    return null;
  }

  // 3. Ignore self-closing tags: '<tag ... /'
  if (tagChunk.trimEnd().endsWith('/')) {
    return null;
  }

  // 4. Ignore if inside attribute string quotes or interpolation braces
  if (isInsideQuotesOrBraces(tagChunk)) {
    return null;
  }

  // 5. Extract tag name
  const match = tagChunk.match(/^<([a-zA-Z0-9_\-:]+)/);
  if (!match) {
    return null;
  }

  const tagName = match[1];

  // 6. Void HTML elements never have closing tags
  if (VOID_ELEMENTS.has(tagName.toLowerCase())) {
    return null;
  }

  return tagName;
}

/**
 * Searches backward in the document to find the innermost unclosed tag for auto-closing on '</'.
 */
export function findMatchingOpenTag(document: vscode.TextDocument, position: vscode.Position): string | null {
  const startLine = Math.max(0, position.line - 50);
  const range = new vscode.Range(new vscode.Position(startLine, 0), position);
  const text = document.getText(range);

  const tagRegex = /<(\/)?([a-zA-Z0-9_\-:]+)(?:[^>]*?)(\/?)>/g;
  const openTags: string[] = [];

  let match: RegExpExecArray | null;
  while ((match = tagRegex.exec(text)) !== null) {
    const isClosing = Boolean(match[1]);
    const tagName = match[2];
    const isSelfClosing = Boolean(match[3]) || VOID_ELEMENTS.has(tagName.toLowerCase());

    if (isSelfClosing) {
      continue;
    }

    if (isClosing) {
      if (openTags.length > 0 && openTags[openTags.length - 1].toLowerCase() === tagName.toLowerCase()) {
        openTags.pop();
      }
    } else {
      openTags.push(tagName);
    }
  }

  return openTags.length > 0 ? openTags[openTags.length - 1] : null;
}

/**
 * Activates automatic tag closing for ERM and EDS.
 * - Typing '>' on an open tag automatically inserts '</tag>'.
 * - Typing '</' automatically completes '</tag>'.
 */
export function activateTagClosing(context: vscode.ExtensionContext) {
  const disposable = vscode.workspace.onDidChangeTextDocument(event => {
    const document = event.document;
    if (document.languageId !== 'eronom-markup') {
      return;
    }

    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document !== document) {
      return;
    }

    if (event.contentChanges.length !== 1) {
      return;
    }

    const change = event.contentChanges[0];

    // Case 1: User typed '>'
    if (change.text === '>') {
      const pos = new vscode.Position(change.range.start.line, change.range.start.character + 1);
      const startLine = Math.max(0, pos.line - 15);
      const range = new vscode.Range(new vscode.Position(startLine, 0), pos);
      const text = document.getText(range);

      if (!text.endsWith('>')) {
        return;
      }

      const textBeforeGt = text.slice(0, -1);
      const tagName = getAutoClosingTag(textBeforeGt);
      if (!tagName) {
        return;
      }

      // Check if already followed by '</tagName>'
      const lineAfter = document.lineAt(pos.line).text.substring(pos.character);
      if (lineAfter.trimStart().startsWith(`</${tagName}>`)) {
        return;
      }

      // Insert closing tag snippet leaving cursor in the middle: <tag>|</tag>
      editor.insertSnippet(new vscode.SnippetString(`$0</${tagName}>`), pos);
      return;
    }

    // Case 2: User typed '/' right after '<' (i.e. '</')
    if (change.text === '/') {
      const pos = new vscode.Position(change.range.start.line, change.range.start.character + 1);
      const lineText = document.lineAt(pos.line).text;
      const textBeforeSlash = lineText.substring(0, pos.character);

      if (!textBeforeSlash.endsWith('</')) {
        return;
      }

      const matchingTag = findMatchingOpenTag(document, change.range.start);
      if (!matchingTag) {
        return;
      }

      const hasTrailingGt = lineText.charAt(pos.character) === '>';
      const snippet = hasTrailingGt ? matchingTag : `${matchingTag}>`;
      editor.insertSnippet(new vscode.SnippetString(snippet), pos);
    }
  });

  context.subscriptions.push(disposable);
}
