// eronom-extension/test/mockVscode.ts - Mock VS Code API for unit tests
import Module from 'module';

export const CompletionItemKind = {
  Text: 0,
  Method: 1,
  Function: 2,
  Constructor: 3,
  Field: 4,
  Variable: 5,
  Class: 6,
  Interface: 7,
  Module: 8,
  Property: 9,
  Unit: 10,
  Value: 11,
  Enum: 12,
  Keyword: 13,
  Snippet: 14,
  Color: 15,
  File: 16,
  Reference: 17,
  Folder: 18,
  EnumMember: 19,
  Constant: 20,
  Struct: 21,
  Event: 22,
  Operator: 23,
  TypeParameter: 24,
  Component: 25,
};

export class CompletionItem {
  detail?: string;
  documentation?: any;
  insertText?: any;
  sortText?: string;
  range?: Range;
  constructor(public label: string, public kind?: number) {}
}

export class SnippetString {
  constructor(public value: string) {}
}

export class MarkdownString {
  value = '';
  constructor(value = '') {
    this.value = value;
  }
  appendMarkdown(str: string) {
    this.value += str;
  }
}

export class Position {
  constructor(public line: number, public character: number) {}
}

export class Range {
  constructor(public start: Position, public end: Position) {}
}

export class Hover {
  constructor(public contents: any, public range?: Range) {}
}

export const workspace = {
  workspaceFolders: undefined,
  getWorkspaceFolder: () => undefined,
};

export const languages = {
  registerCompletionItemProvider: () => ({ dispose: () => {} }),
  registerHoverProvider: () => ({ dispose: () => {} }),
};

export const commands = {
  registerCommand: () => ({ dispose: () => {} }),
};

export const window = {
  terminals: [],
  createTerminal: () => ({ show: () => {}, sendText: () => {} }),
  showErrorMessage: () => {},
  showWarningMessage: () => {},
  showInputBox: () => {},
};

export const mockVscode = {
  CompletionItemKind,
  CompletionItem,
  SnippetString,
  MarkdownString,
  Position,
  Range,
  Hover,
  workspace,
  languages,
  commands,
  window,
};

// Hook into module require
const origRequire = (Module as any).prototype.require;
(Module as any).prototype.require = function (id: string) {
  if (id === 'vscode') {
    return mockVscode;
  }
  return origRequire.apply(this, arguments);
};
