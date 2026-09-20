// eronom-extension/src/hoverProvider.ts - ERM & EDS Hover Documentation Provider
import * as vscode from 'vscode';
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
  BUTTON_VARIANTS,
  BUTTON_SIZES,
  BADGE_STATUSES,
  TITLE_ORDERS,
  AS_ELEMENTS,
  EdTokenValue,
} from './edsData';

export class ErmHoverProvider implements vscode.HoverProvider {
  public provideHover(
    document: vscode.TextDocument,
    position: vscode.Position,
    token: vscode.CancellationToken
  ): vscode.ProviderResult<vscode.Hover> {
    const range = document.getWordRangeAtPosition(position, /[a-zA-Z0-9_\-:]+/);
    if (!range) {
      return null;
    }

    const word = document.getText(range);
    const line = document.lineAt(position).text;

    // 1. Check if word is an EDS Primitive (e.g. <Button, <Stack, <Box, etc.)
    if (EDS_PRIMITIVES[word]) {
      const primitive = EDS_PRIMITIVES[word];
      const md = new vscode.MarkdownString();
      md.appendMarkdown(`### \`<${primitive.name}>\` (Eronom Design System)\n\n`);
      md.appendMarkdown(`${primitive.description}\n\n`);
      md.appendMarkdown(`- **Default HTML Tag**: \`<${primitive.defaultTag}>\`\n`);
      md.appendMarkdown(`- **Dual-Theme**: Supported natively with CSS \`light-dark()\`\n\n`);
      md.appendMarkdown(`**Key Props**:\n`);
      for (const [propName, propInfo] of Object.entries(primitive.props).slice(0, 8)) {
        md.appendMarkdown(`- \`${propName}\` (\`${propInfo.type}\`): ${propInfo.description}\n`);
      }
      return new vscode.Hover(md, range);
    }

    // 2. Check if hovering on a prop name inside a tag: e.g. <Button variant="primary" or <Stack gap="md"
    const propHover = this.getPropDocumentation(word, line, position.character);
    if (propHover) {
      return new vscode.Hover(propHover, range);
    }

    // 3. Check if hovering on a token value (e.g. "canvas", "card", "subtle", "title-lg", "primary")
    const tokenHover = this.getTokenDocumentation(word);
    if (tokenHover) {
      return new vscode.Hover(tokenHover, range);
    }

    return null;
  }

  private getPropDocumentation(word: string, line: string, cursorChar: number): vscode.MarkdownString | null {
    // Check if word is followed by '=' or preceded by whitespace inside a tag
    const beforeCursor = line.substring(0, cursorChar);
    const lastTagOpen = beforeCursor.lastIndexOf('<');
    if (lastTagOpen === -1) {
      return null;
    }

    const tagChunk = beforeCursor.substring(lastTagOpen);
    if (tagChunk.startsWith('</') || tagChunk.includes('>')) {
      return null;
    }

    const tagNameMatch = tagChunk.match(/^<([a-zA-Z0-9_\-:]+)/);
    const tagName = tagNameMatch ? tagNameMatch[1] : '';

    const primitive = EDS_PRIMITIVES[tagName];
    const props = primitive ? { ...primitive.props } : { ...COMMON_LAYOUT_PROPS };

    if (props[word]) {
      const prop = props[word];
      const md = new vscode.MarkdownString();
      md.appendMarkdown(`### EDS Prop: \`${word}\`\n\n`);
      md.appendMarkdown(`**Type**: \`${prop.type}\`\n\n`);
      md.appendMarkdown(`${prop.description}\n\n`);
      if (prop.values && prop.values.length > 0) {
        md.appendMarkdown(`**Supported Values**:\n`);
        for (const v of prop.values) {
          md.appendMarkdown(`- \`${v.name}\`: ${v.description}\n`);
        }
      }
      return md;
    }

    return null;
  }

  private getTokenDocumentation(word: string): vscode.MarkdownString | null {
    const allTokenLists: { category: string; tokens: EdTokenValue[] }[] = [
      { category: 'Surface & Background Colors', tokens: SURFACE_COLOR_TOKENS },
      { category: 'Text Colors', tokens: TEXT_COLOR_TOKENS },
      { category: 'Spacings', tokens: SPACING_TOKENS },
      { category: 'Button Variants', tokens: BUTTON_VARIANTS },
      { category: 'Button Sizes', tokens: BUTTON_SIZES },
      { category: 'Button Palette Colors', tokens: BUTTON_PALETTE_COLORS },
      { category: 'Badge Statuses', tokens: BADGE_STATUSES },
      { category: 'Typography Scale', tokens: TEXT_VARIANTS },
      { category: 'Font Sizes', tokens: FONT_SIZES },
      { category: 'Font Weights', tokens: FONT_WEIGHTS },
      { category: 'Borders', tokens: BORDER_TOKENS },
      { category: 'Corner Radius', tokens: RADIUS_TOKENS },
      { category: 'Elevation Shadows', tokens: SHADOW_TOKENS },
      { category: 'Semantic Elements', tokens: AS_ELEMENTS },
      { category: 'Heading Orders', tokens: TITLE_ORDERS },
    ];

    for (const list of allTokenLists) {
      const found = list.tokens.find(t => t.name === word);
      if (found) {
        const md = new vscode.MarkdownString();
        md.appendMarkdown(`### EDS Intent Token: \`${found.name}\`\n\n`);
        md.appendMarkdown(`**Category**: ${list.category}\n\n`);
        md.appendMarkdown(`${found.description}\n\n`);
        if (found.cssOutput) {
          md.appendMarkdown(`**Compiled CSS Output**:\n\`\`\`css\n${found.cssOutput}\n\`\`\`\n`);
        }
        return md;
      }
    }

    return null;
  }
}
