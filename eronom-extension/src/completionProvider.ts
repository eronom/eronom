// eronom-extension/src/completionProvider.ts - ERM & EDS Auto-Suggestions Provider
import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import {
  EDS_PRIMITIVES,
  COMMON_LAYOUT_PROPS,
  SPACING_TOKENS,
  SURFACE_COLOR_TOKENS,
  TEXT_COLOR_TOKENS,
  BUTTON_PALETTE_COLORS,
  BORDER_TOKENS,
  RADIUS_TOKENS,
  SHADOW_TOKENS,
  TEXT_VARIANTS,
  FONT_SIZES,
  FONT_WEIGHTS,
  ALIGN_TOKENS,
  TEXT_ALIGN_TOKENS,
  JUSTIFY_TOKENS,
  MAX_WIDTH_TOKENS,
  MIN_HEIGHT_TOKENS,
  BUTTON_VARIANTS,
  BUTTON_SIZES,
  BADGE_STATUSES,
  TITLE_ORDERS,
  AS_ELEMENTS,
  EdTokenValue,
  EdPropInfo,
} from './edsData';

export class ErmCompletionItemProvider implements vscode.CompletionItemProvider {
  public provideCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
    token: vscode.CancellationToken,
    context: vscode.CompletionContext
  ): vscode.ProviderResult<vscode.CompletionItem[] | vscode.CompletionList> {
    const linePrefix = document.lineAt(position).text.substring(0, position.character);

    // 1. Check if cursor is inside an attribute value: e.g. bg="|" or variant='|'
    const attrValueMatch = linePrefix.match(/([a-zA-Z0-9_\-:]+)\s*=\s*["']([^"']*)$/);
    if (attrValueMatch) {
      const attrName = attrValueMatch[1];
      const tagInfo = this.findEnclosingTag(document, position);
      return this.getValueCompletions(attrName, tagInfo?.tagName);
    }

    // 2. Check if cursor is inside an open tag: e.g. <Button | or <Stack gap="md" |
    const tagInfo = this.findEnclosingTag(document, position);
    if (tagInfo && tagInfo.isInsideTag) {
      return this.getAttributeCompletions(tagInfo.tagName, tagInfo.existingAttrs);
    }

    // 3. Check if cursor is starting a tag: e.g. <| or <B|
    const tagStartMatch = linePrefix.match(/<([a-zA-Z0-9_\-:]*)$/);
    if (tagStartMatch) {
      return this.getTagCompletions(document, position, tagStartMatch);
    }

    // 4. Default / top-level completions (ERM directives, control blocks, snippets, and tag completions)
    return this.getGeneralCompletions(document);
  }

  /**
   * Scans backward to determine if the cursor is currently inside an opening HTML/EDS tag.
   */
  private findEnclosingTag(
    document: vscode.TextDocument,
    position: vscode.Position
  ): { tagName: string; isInsideTag: boolean; existingAttrs: Set<string> } | null {
    // Look up to 15 lines back to handle multiline tags
    const startLine = Math.max(0, position.line - 15);
    const range = new vscode.Range(new vscode.Position(startLine, 0), position);
    const text = document.getText(range);

    // Find the last '<' that isn't '</' or '<!--'
    const lastOpenTagIdx = text.lastIndexOf('<');
    if (lastOpenTagIdx === -1) {
      return null;
    }

    const tagChunk = text.substring(lastOpenTagIdx);

    // If it's a closing tag or comment or has already been closed with '>', ignore
    if (tagChunk.startsWith('</') || tagChunk.startsWith('<!--')) {
      return null;
    }

    // If '>' appears before the cursor position in this tag chunk, it's already closed
    const closeAngleIdx = tagChunk.indexOf('>');
    if (closeAngleIdx !== -1 && closeAngleIdx < tagChunk.length - 1) {
      return null;
    }

    // Extract tag name: must be followed by whitespace to be inside attribute context
    const tagNameMatch = tagChunk.match(/^<([a-zA-Z0-9_\-:]+)\s+/);
    if (!tagNameMatch) {
      return null;
    }

    const tagName = tagNameMatch[1];

    // Collect already present attributes
    const existingAttrs = new Set<string>();
    const attrRegex = /([a-zA-Z0-9_\-:]+)(?:\s*=\s*(?:["'][^"']*["']|\{[^}]*\}|[^\s>]+))?/g;
    let match: RegExpExecArray | null;
    // Skip the tag name itself
    const attrsText = tagChunk.substring(tagNameMatch[0].length);
    while ((match = attrRegex.exec(attrsText)) !== null) {
      existingAttrs.add(match[1]);
    }

    return {
      tagName,
      isInsideTag: true,
      existingAttrs,
    };
  }

  /**
   * Provides suggestions for EDS token values when inside attribute quotes (e.g. bg="|" or gap="|").
   */
  private getValueCompletions(attrName: string, tagName?: string): vscode.CompletionItem[] {
    const items: vscode.CompletionItem[] = [];

    const addTokens = (tokens: EdTokenValue[], kind = vscode.CompletionItemKind.Value, priority = '0') => {
      tokens.forEach((token, idx) => {
        const item = new vscode.CompletionItem(token.name, kind);
        item.detail = token.description;
        if (token.cssOutput) {
          item.documentation = new vscode.MarkdownString(
            `**Token**: \`${token.name}\`\n\n${token.description}\n\n**Compiled CSS**:\n\`\`\`css\n${token.cssOutput}\n\`\`\``
          );
        } else {
          item.documentation = new vscode.MarkdownString(`**Token**: \`${token.name}\`\n\n${token.description}`);
        }
        item.sortText = `${priority}_${String(idx).padStart(3, '0')}`;
        items.push(item);
      });
    };

    switch (attrName) {
      case 'bg':
        addTokens(SURFACE_COLOR_TOKENS, vscode.CompletionItemKind.Color);
        break;

      case 'c':
      case 'color':
        if (tagName === 'Button') {
          addTokens(BUTTON_PALETTE_COLORS, vscode.CompletionItemKind.Color, '0');
          addTokens(TEXT_COLOR_TOKENS, vscode.CompletionItemKind.Color, '1');
        } else {
          addTokens(TEXT_COLOR_TOKENS, vscode.CompletionItemKind.Color);
        }
        break;

      case 'gap':
      case 'p':
      case 'pt':
      case 'pb':
      case 'pl':
      case 'pr':
      case 'm':
      case 'mx':
      case 'my':
      case 'mt':
      case 'mb':
      case 'ml':
      case 'mr':
        addTokens(SPACING_TOKENS, vscode.CompletionItemKind.Unit);
        break;

      case 'px':
      case 'py': {
        addTokens(SPACING_TOKENS, vscode.CompletionItemKind.Unit, '0');
        const customPresets: EdTokenValue[] = [
          { name: '24px', description: 'Custom button/container 24px horizontal padding' },
          { name: '18px', description: 'Custom 18px horizontal padding' },
          { name: '10px', description: 'Custom button 10px vertical padding' },
          { name: '1.5rem', description: 'Custom 1.5rem padding' },
          { name: '1rem', description: 'Custom 1rem padding' },
        ];
        addTokens(customPresets, vscode.CompletionItemKind.Snippet, '1');
        break;
      }

      case 'variant':
        if (tagName === 'Button') {
          addTokens(BUTTON_VARIANTS, vscode.CompletionItemKind.EnumMember);
        } else {
          addTokens(TEXT_VARIANTS, vscode.CompletionItemKind.EnumMember);
        }
        break;

      case 'size':
        if (tagName === 'Button') {
          addTokens(BUTTON_SIZES, vscode.CompletionItemKind.EnumMember);
        } else {
          addTokens(FONT_SIZES, vscode.CompletionItemKind.Unit);
        }
        break;

      case 'fz':
        addTokens(FONT_SIZES, vscode.CompletionItemKind.Unit);
        break;

      case 'fw':
      case 'weight':
        addTokens(FONT_WEIGHTS, vscode.CompletionItemKind.Unit);
        break;

      case 'status':
        addTokens(BADGE_STATUSES, vscode.CompletionItemKind.EnumMember);
        break;

      case 'order':
        addTokens(TITLE_ORDERS, vscode.CompletionItemKind.EnumMember);
        break;

      case 'align':
        addTokens(ALIGN_TOKENS, vscode.CompletionItemKind.EnumMember, '0');
        addTokens(TEXT_ALIGN_TOKENS, vscode.CompletionItemKind.EnumMember, '1');
        break;

      case 'ta':
        addTokens(TEXT_ALIGN_TOKENS, vscode.CompletionItemKind.EnumMember);
        break;

      case 'justify':
        addTokens(JUSTIFY_TOKENS, vscode.CompletionItemKind.EnumMember);
        break;

      case 'direction':
        addTokens([
          { name: 'row', description: 'Horizontal layout (flex-direction: row)' },
          { name: 'col', description: 'Vertical layout (flex-direction: column)' },
        ], vscode.CompletionItemKind.EnumMember);
        break;

      case 'wrap':
        addTokens([
          { name: 'wrap', description: 'Allow items to wrap (flex-wrap: wrap)' },
          { name: 'nowrap', description: 'Single line layout (flex-wrap: nowrap)' },
          { name: 'wrap-reverse', description: 'Wrap items in reverse order' },
        ], vscode.CompletionItemKind.EnumMember);
        break;

      case 'border':
      case 'bd':
      case 'borderTop':
      case 'borderBottom':
      case 'borderLeft':
      case 'borderRight':
        addTokens(BORDER_TOKENS, vscode.CompletionItemKind.EnumMember);
        break;

      case 'radius':
      case 'bdrs':
        addTokens(RADIUS_TOKENS, vscode.CompletionItemKind.Unit);
        break;

      case 'shadow':
        addTokens(SHADOW_TOKENS, vscode.CompletionItemKind.EnumMember);
        break;

      case 'maw':
      case 'maxW':
        addTokens(MAX_WIDTH_TOKENS, vscode.CompletionItemKind.Unit);
        break;

      case 'mih':
      case 'minH':
        addTokens(MIN_HEIGHT_TOKENS, vscode.CompletionItemKind.Unit);
        break;

      case 'w':
      case 'width':
      case 'h':
      case 'height':
        addTokens([
          { name: 'full', description: 'Full size (100%)' },
          { name: 'screen', description: 'Full viewport screen (100vw or 100dvh)' },
          { name: 'auto', description: 'Automatic dimension' },
          { name: 'fit', description: 'Fit content sizing' },
        ], vscode.CompletionItemKind.Unit);
        break;

      case 'as':
        addTokens(AS_ELEMENTS, vscode.CompletionItemKind.Keyword);
        break;

      case 'tt':
        addTokens([
          { name: 'uppercase', description: 'Transform text to uppercase' },
          { name: 'lowercase', description: 'Transform text to lowercase' },
          { name: 'capitalize', description: 'Capitalize words' },
          { name: 'none', description: 'No text transform' },
        ], vscode.CompletionItemKind.EnumMember);
        break;

      case 'td':
        addTokens([
          { name: 'underline', description: 'Underline text' },
          { name: 'line-through', description: 'Strikethrough text' },
          { name: 'none', description: 'No text decoration' },
        ], vscode.CompletionItemKind.EnumMember);
        break;

      case 'pos':
        addTokens([
          { name: 'relative', description: 'Position relative' },
          { name: 'absolute', description: 'Position absolute' },
          { name: 'fixed', description: 'Position fixed' },
          { name: 'sticky', description: 'Position sticky' },
        ], vscode.CompletionItemKind.EnumMember);
        break;

      case 'flex':
        addTokens([
          { name: '1', description: 'flex: 1 (take remaining container space)' },
          { name: 'auto', description: 'flex: auto' },
          { name: 'none', description: 'flex: none' },
          { name: 'initial', description: 'flex: 0 1 auto' },
        ], vscode.CompletionItemKind.EnumMember);
        break;

      case 'target':
        addTokens([
          { name: '_blank', description: 'Open in a new tab or window' },
          { name: '_self', description: 'Open in the current tab or window' },
        ], vscode.CompletionItemKind.EnumMember);
        break;

      case 'type':
        addTokens([
          { name: 'button', description: 'Standard clickable button' },
          { name: 'submit', description: 'Submit enclosing form' },
          { name: 'reset', description: 'Reset form values' },
        ], vscode.CompletionItemKind.EnumMember);
        break;
    }

    return items;
  }

  /**
   * Provides context-aware prop completions when inside an open tag (e.g. <Button | or <Stack |).
   */
  private getAttributeCompletions(tagName: string, existingAttrs: Set<string>): vscode.CompletionItem[] {
    const items: vscode.CompletionItem[] = [];
    const primitive = EDS_PRIMITIVES[tagName];
    const propsToSuggest: Record<string, EdPropInfo> = primitive ? { ...primitive.props } : { ...COMMON_LAYOUT_PROPS };

    // Also add event handlers and common HTML attributes
    const commonHtmlAttrs: Record<string, EdPropInfo> = {
      onClick: { name: 'onClick', type: 'Function', description: 'Click event handler callback' },
      onInput: { name: 'onInput', type: 'Function', description: 'Input value change callback' },
      onChange: { name: 'onChange', type: 'Function', description: 'Change event callback' },
      onSubmit: { name: 'onSubmit', type: 'Function', description: 'Form submit event callback' },
      class: { name: 'class', type: 'string', description: 'Custom CSS class list' },
      id: { name: 'id', type: 'string', description: 'Element unique identifier' },
      style: { name: 'style', type: 'string', description: 'Inline CSS styles' },
    };

    const mergedProps = { ...propsToSuggest, ...commonHtmlAttrs };

    let order = 0;
    for (const [propName, propInfo] of Object.entries(mergedProps)) {
      if (existingAttrs.has(propName)) {
        continue;
      }

      const item = new vscode.CompletionItem(propName, vscode.CompletionItemKind.Property);
      item.detail = `(${propInfo.type}) ${propInfo.description}`;

      let docMarkdown = `### \`${propName}\`\n\n**Type**: \`${propInfo.type}\`\n\n${propInfo.description}\n\n`;
      if (propInfo.values && propInfo.values.length > 0) {
        docMarkdown += `**Allowed Values**:\n` + propInfo.values.map(v => `- \`${v.name}\`: ${v.description}`).join('\n');
      }
      item.documentation = new vscode.MarkdownString(docMarkdown);

      if (propInfo.isBoolean) {
        item.insertText = propName;
      } else if (propName.startsWith('on')) {
        item.insertText = new vscode.SnippetString(`${propName}={$1}`);
      } else {
        item.insertText = new vscode.SnippetString(`${propName}="$1"`);
      }

      item.sortText = String(order++).padStart(3, '0');
      items.push(item);
    }

    return items;
  }

  /**
   * Provides tag completions when user types '<' (EDS primitives, HTML tags, local components)
   * or when typing tag names directly.
   */
  private getTagCompletions(
    document: vscode.TextDocument,
    position?: vscode.Position,
    tagStartMatch?: RegExpMatchArray
  ): vscode.CompletionItem[] {
    const items: vscode.CompletionItem[] = [];

    let replaceRange: vscode.Range | undefined;
    if (tagStartMatch && position && tagStartMatch.index !== undefined) {
      const lineText = document.lineAt(position).text;
      const startChar = tagStartMatch.index;
      let endChar = position.character;
      if (lineText.charAt(endChar) === '>') {
        endChar += 1;
      }
      replaceRange = new vscode.Range(
        new vscode.Position(position.line, startChar),
        new vscode.Position(position.line, endChar)
      );
    }

    const applyRange = (item: vscode.CompletionItem) => {
      if (replaceRange) {
        item.range = replaceRange;
      }
      return item;
    };

    // 1. EDS Primitives
    let sortIdx = 0;
    for (const [name, primitive] of Object.entries(EDS_PRIMITIVES)) {
      const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Class);
      item.detail = `EDS Primitive <${name}>`;
      item.documentation = new vscode.MarkdownString(
        `### \`<${name}>\` (Eronom Design System)\n\n${primitive.description}\n\n` +
        `**Default HTML Element**: \`<${primitive.defaultTag}>\`\n\n` +
        `**Key Props**: ${Object.keys(primitive.props).slice(0, 8).join(', ')}...`
      );
      item.insertText = new vscode.SnippetString(primitive.snippet);
      item.sortText = `0_${String(sortIdx++).padStart(2, '0')}`;
      items.push(applyRange(item));
    }

    // 2. Common Semantic HTML Tags
    const semanticTags = [
      'div', 'span', 'p', 'a', 'button', 'header', 'footer', 'section', 'nav', 'main',
      'article', 'aside', 'ul', 'li', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'code', 'pre',
      'form', 'input', 'label', 'img', 'table', 'tr', 'td', 'th',
    ];
    semanticTags.forEach(tag => {
      const item = new vscode.CompletionItem(tag, vscode.CompletionItemKind.Property);
      item.detail = `HTML <${tag}>`;
      if (['input', 'img', 'br', 'hr'].includes(tag)) {
        item.insertText = new vscode.SnippetString(`<${tag} $0/>`);
      } else {
        item.insertText = new vscode.SnippetString(`<${tag}>$0</${tag}>`);
      }
      item.sortText = `1_${tag}`;
      items.push(applyRange(item));
    });

    // 3. ERM Built-in Blocks
    const scriptItem = new vscode.CompletionItem('script', vscode.CompletionItemKind.Module);
    scriptItem.detail = 'Eronom Script Block';
    scriptItem.insertText = new vscode.SnippetString('<script>\n\t$0\n</script>');
    scriptItem.sortText = '0_script';
    items.push(applyRange(scriptItem));

    const styleItem = new vscode.CompletionItem('style', vscode.CompletionItemKind.Module);
    styleItem.detail = 'Eronom Scoped Style Block';
    styleItem.insertText = new vscode.SnippetString('<style>\n\t$0\n</style>');
    styleItem.sortText = '0_style';
    items.push(applyRange(styleItem));

    const slotItem = new vscode.CompletionItem('slot', vscode.CompletionItemKind.Keyword);
    slotItem.detail = 'ERM Component Slot';
    slotItem.insertText = new vscode.SnippetString('<slot />');
    items.push(applyRange(slotItem));

    // 4. Local workspace components from components/ or app/components/
    this.findWorkspaceComponents(document).forEach(compName => {
      const item = new vscode.CompletionItem(compName, vscode.CompletionItemKind.Component);
      item.detail = `Local Component <${compName} />`;
      item.insertText = new vscode.SnippetString(`<${compName} $0/>`);
      item.sortText = `0_comp_${compName}`;
      items.push(applyRange(item));
    });

    return items;
  }

  /**
   * Scans the workspace to find local .erm component names (e.g. Header, Footer).
   */
  private findWorkspaceComponents(document: vscode.TextDocument): string[] {
    const components: string[] = [];
    try {
      const workspaceFolder = vscode.workspace.getWorkspaceFolder(document.uri);
      if (!workspaceFolder) {
        return components;
      }

      const rootPath = workspaceFolder.uri.fsPath;
      const searchDirs = [
        path.join(rootPath, 'components'),
        path.join(rootPath, 'app', 'components'),
        path.join(rootPath, 'app', 'layouts'),
      ];

      for (const dir of searchDirs) {
        if (fs.existsSync(dir)) {
          const files = fs.readdirSync(dir);
          for (const file of files) {
            if (file.endsWith('.erm')) {
              const compName = path.basename(file, '.erm');
              if (compName && compName[0] === compName[0].toUpperCase()) {
                components.push(compName);
              }
            }
          }
        }
      }
    } catch {
      // Ignore directory scan errors
    }
    return components;
  }

  /**
   * Provides top-level ERM reactivity, control flow, template snippets,
   * and tag completions when typed directly without '<'.
   */
  private getGeneralCompletions(document: vscode.TextDocument): vscode.CompletionItem[] {
    const items: vscode.CompletionItem[] = [];

    // Tag and primitive completions (so typing Card, Box, div without '<' works)
    items.push(...this.getTagCompletions(document));

    // useState
    const useStateItem = new vscode.CompletionItem('useState', vscode.CompletionItemKind.Function);
    useStateItem.detail = 'Reactive State Variable';
    useStateItem.documentation = new vscode.MarkdownString('Creates a reactive state variable that updates the DOM on change.\n\n```javascript\nlet count = useState(0);\n```');
    useStateItem.insertText = new vscode.SnippetString('let ${1:stateName} = useState(${2:initialValue});');
    items.push(useStateItem);

    // Control Blocks
    const ifItem = new vscode.CompletionItem('if block', vscode.CompletionItemKind.Snippet);
    ifItem.detail = 'ERM Conditional Block';
    ifItem.insertText = new vscode.SnippetString('if ${1:condition} {\n\t$0\n}');
    items.push(ifItem);

    const ifElseItem = new vscode.CompletionItem('if else block', vscode.CompletionItemKind.Snippet);
    ifElseItem.detail = 'ERM If-Else Block';
    ifElseItem.insertText = new vscode.SnippetString('if ${1:condition} {\n\t$2\n} else {\n\t$0\n}');
    items.push(ifElseItem);

    const forItem = new vscode.CompletionItem('for loop', vscode.CompletionItemKind.Snippet);
    forItem.detail = 'ERM For Loop Block';
    forItem.insertText = new vscode.SnippetString('for ${1:item} in ${2:list} {\n\t$0\n}');
    items.push(forItem);

    const foriItem = new vscode.CompletionItem('for index loop', vscode.CompletionItemKind.Snippet);
    foriItem.detail = 'ERM For Loop Block with Index';
    foriItem.insertText = new vscode.SnippetString('for ${1:item}, ${2:i} in ${3:list} {\n\t$0\n}');
    items.push(foriItem);

    return items;
  }
}
