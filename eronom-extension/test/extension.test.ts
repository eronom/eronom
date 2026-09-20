import "./mockVscode";
import { test, describe } from "node:test";
import assert from "node:assert";
import * as fs from "fs";
import * as path from "path";
import { ErmCompletionItemProvider } from "../src/completionProvider";
import { ErmHoverProvider } from "../src/hoverProvider";
import { getAutoClosingTag, findMatchingOpenTag } from "../src/tagClosing";
import { EDS_PRIMITIVES, SPACING_TOKENS, SURFACE_COLOR_TOKENS } from "../src/edsData";
import { Position, Range } from "./mockVscode";

const extensionDir = path.resolve(__dirname, "..");

describe("Eronom Extension Configuration Tests", () => {
  const packageJsonPath = path.join(extensionDir, "package.json");

  test("package.json exists and is valid JSON", () => {
    assert.strictEqual(fs.existsSync(packageJsonPath), true);
    const content = fs.readFileSync(packageJsonPath, "utf8");
    const pkg = JSON.parse(content);
    assert.strictEqual(pkg.name, "eronom-extension");
    assert.strictEqual(pkg.main, "./dist/extension.js");
  });

  test("package.json registers .eds and .erm with activationEvents", () => {
    const pkg = JSON.parse(fs.readFileSync(packageJsonPath, "utf8"));
    assert.ok(Array.isArray(pkg.activationEvents), "activationEvents should be defined");
    assert.ok(pkg.activationEvents.includes("onLanguage:eronom-markup"));

    const markupLang = pkg.contributes.languages.find(
      (l: any) => l.id === "eronom-markup"
    );
    assert.ok(markupLang, "eronom-markup language should exist");
    assert.ok(markupLang.extensions.includes(".erm"), ".erm extension should be registered");
    assert.ok(markupLang.extensions.includes(".eds"), ".eds extension should be registered");
    assert.ok(markupLang.aliases.includes("eds"), "eds alias should be registered");
  });

  test("All contributed files exist", () => {
    const content = fs.readFileSync(packageJsonPath, "utf8");
    const pkg = JSON.parse(content);

    // Check languages configurations
    const languages = pkg.contributes.languages;
    assert.ok(Array.isArray(languages));
    for (const lang of languages) {
      const configPath = path.resolve(extensionDir, lang.configuration);
      assert.strictEqual(fs.existsSync(configPath), true);
      assert.doesNotThrow(() => JSON.parse(fs.readFileSync(configPath, "utf-8")));
    }

    // Check grammars
    const grammars = pkg.contributes.grammars;
    assert.ok(Array.isArray(grammars));
    for (const grammar of grammars) {
      const grammarPath = path.resolve(extensionDir, grammar.path);
      assert.strictEqual(fs.existsSync(grammarPath), true);
      assert.doesNotThrow(() => JSON.parse(fs.readFileSync(grammarPath, "utf-8")));
    }

    // Check snippets
    const snippets = pkg.contributes.snippets;
    assert.ok(Array.isArray(snippets));
    for (const snippet of snippets) {
      const snippetPath = path.resolve(extensionDir, snippet.path);
      assert.strictEqual(fs.existsSync(snippetPath), true);
      assert.doesNotThrow(() => JSON.parse(fs.readFileSync(snippetPath, "utf-8")));
    }
  });

  test("Compiled output exists", () => {
    const distPath = path.join(extensionDir, "dist", "extension.js");
    assert.strictEqual(fs.existsSync(distPath), true);
  });

  test("ERM language configuration does not auto-close angle brackets", () => {
    const configPath = path.join(extensionDir, "erm-language-configuration.json");
    const config = JSON.parse(fs.readFileSync(configPath, "utf8"));
    const hasAngleAutoClose = config.autoClosingPairs.some(
      (p: any) => p.open === "<" && p.close === ">"
    );
    assert.strictEqual(hasAngleAutoClose, false, "autoClosingPairs should NOT include < > to prevent rogue extra closing tags");

    const hasAngleSurround = config.surroundingPairs.some(
      (p: any) => p[0] === "<" && p[1] === ">"
    );
    assert.ok(hasAngleSurround, "surroundingPairs should include < >");
  });

  test("ERM snippets contain EDS primitives", () => {
    const snippetPath = path.join(extensionDir, "snippets", "erm.code-snippets");
    const snippets = JSON.parse(fs.readFileSync(snippetPath, "utf8"));
    assert.ok(snippets["EDS Button"], "EDS Button snippet should exist");
    assert.ok(snippets["EDS Card"], "EDS Card snippet should exist");
    assert.ok(snippets["EDS Stack"], "EDS Stack snippet should exist");
    assert.ok(snippets["EDS Hero Page"], "EDS Hero Page snippet should exist");
  });

  test("ERM grammar defines tag-open and tag-close without invalid.illegal", () => {
    const ermGrammarPath = path.join(extensionDir, "syntaxes", "erm.tmLanguage.json");
    const ermGrammar = JSON.parse(fs.readFileSync(ermGrammarPath, "utf8"));
    assert.ok(ermGrammar.repository["tag-open"], "tag-open pattern should exist");
    assert.ok(ermGrammar.repository["tag-close"], "tag-close pattern should exist");
    assert.strictEqual(
      ermGrammar.repository["tag-open"].beginCaptures["2"].name,
      "entity.name.tag.html"
    );
    assert.strictEqual(
      ermGrammar.repository["tag-close"].captures["2"].name,
      "entity.name.tag.html"
    );
  });
});

describe("ERM & EDS Auto-Suggestion Provider Tests", () => {
  const provider = new ErmCompletionItemProvider();

  function createMockDoc(lines: string[]) {
    return {
      lineAt: (pos: any) => {
        const lineIdx = typeof pos === 'number' ? pos : pos.line;
        return { text: lines[lineIdx] || "" };
      },
      getText: (range: any) => {
        const start = range.start.line;
        const end = range.end.line;
        let res = "";
        for (let i = start; i <= end; i++) {
          if (i === end) {
            res += lines[i].substring(0, range.end.character);
          } else {
            res += lines[i] + "\n";
          }
        }
        return res;
      },
      uri: { fsPath: "/workspace/test.erm" },
    } as any;
  }

  test("Suggests EDS primitives when opening tag with '<'", () => {
    const doc = createMockDoc(["<"]);
    const items = provider.provideCompletionItems(
      doc,
      new Position(0, 1),
      {} as any,
      {} as any
    ) as any[];

    assert.ok(Array.isArray(items), "Should return an array of completions");
    const labels = items.map((i: any) => i.label);
    assert.ok(labels.includes("Button"), "Should suggest Button");
    assert.ok(labels.includes("Stack"), "Should suggest Stack");
    assert.ok(labels.includes("Card"), "Should suggest Card");

    const buttonItem = items.find((i: any) => i.label === "Button");
    assert.ok(buttonItem, "Button item should exist");
    assert.ok(buttonItem.range, "Should have replaceRange covering '<'");
    assert.strictEqual(buttonItem.range.start.character, 0);
    assert.strictEqual(buttonItem.range.end.character, 1);
    assert.ok(buttonItem.insertText.value.startsWith("<Button"), "Should start with <Button");
  });

  test("Suggests '<div' and replaces whole tag without duplicate brackets", () => {
    const doc = createMockDoc(["<div"]);
    const items = provider.provideCompletionItems(
      doc,
      new Position(0, 4),
      {} as any,
      {} as any
    ) as any[];

    const divItem = items.find((i: any) => i.label === "div");
    assert.ok(divItem, "div item should exist");
    assert.ok(divItem.range, "div should have replaceRange");
    assert.strictEqual(divItem.range.start.character, 0, "Range should start before '<'");
    assert.strictEqual(divItem.range.end.character, 4, "Range should end at cursor");
    assert.strictEqual(divItem.insertText.value, "<div>$0</div>");
  });

  test("Swallows trailing '>' if user or auto-close inserted it", () => {
    const doc = createMockDoc(["<div>"]);
    const items = provider.provideCompletionItems(
      doc,
      new Position(0, 4), // cursor between 'v' and '>'
      {} as any,
      {} as any
    ) as any[];

    const divItem = items.find((i: any) => i.label === "div");
    assert.ok(divItem, "div item should exist");
    assert.ok(divItem.range, "div should have replaceRange");
    assert.strictEqual(divItem.range.start.character, 0);
    assert.strictEqual(divItem.range.end.character, 5, "Range should swallow trailing '>'");
    assert.strictEqual(divItem.insertText.value, "<div>$0</div>");
  });

  test("Suggests tags when typing directly without '<' (e.g. Card)", () => {
    const doc = createMockDoc(["Card"]);
    const items = provider.provideCompletionItems(
      doc,
      new Position(0, 4),
      {} as any,
      {} as any
    ) as any[];

    const cardItem = items.find((i: any) => i.label === "Card");
    assert.ok(cardItem, "Card item should exist");
    assert.ok(cardItem.insertText.value.startsWith("<Card"), "Should insert <Card snippet");
  });

  test("Suggests Button props inside <Button ", () => {
    const doc = createMockDoc(["<Button "]);
    const items = provider.provideCompletionItems(
      doc,
      new Position(0, 8),
      {} as any,
      {} as any
    ) as any[];

    assert.ok(Array.isArray(items));
    const labels = items.map((i: any) => i.label);
    assert.ok(labels.includes("variant"), "Should suggest variant prop");
    assert.ok(labels.includes("size"), "Should suggest size prop");
    assert.ok(labels.includes("color"), "Should suggest color prop");
    assert.ok(labels.includes("onClick"), "Should suggest onClick prop");
    assert.ok(labels.includes("px"), "Should suggest px prop");
  });

  test("Filters out existing props from attribute suggestions", () => {
    const doc = createMockDoc(['<Button variant="primary" ']);
    const items = provider.provideCompletionItems(
      doc,
      new Position(0, 26),
      {} as any,
      {} as any
    ) as any[];

    const labels = items.map((i: any) => i.label);
    assert.strictEqual(
      labels.includes("variant"),
      false,
      "Should NOT suggest variant when already present"
    );
    assert.ok(labels.includes("size"), "Should still suggest size");
    assert.ok(labels.includes("color"), "Should still suggest color");
  });

  test("Suggests surface colors inside bg=\"", () => {
    const doc = createMockDoc(['<Box bg="']);
    const items = provider.provideCompletionItems(
      doc,
      new Position(0, 9),
      {} as any,
      {} as any
    ) as any[];

    const labels = items.map((i: any) => i.label);
    assert.ok(labels.includes("canvas"), "Should suggest canvas");
    assert.ok(labels.includes("card"), "Should suggest card");
    assert.ok(labels.includes("body"), "Should suggest body");
    assert.ok(labels.includes("primary"), "Should suggest primary");
  });

  test("Suggests spacing tokens inside gap=\"", () => {
    const doc = createMockDoc(['<Stack gap="']);
    const items = provider.provideCompletionItems(
      doc,
      new Position(0, 12),
      {} as any,
      {} as any
    ) as any[];

    const labels = items.map((i: any) => i.label);
    assert.ok(labels.includes("none"), "Should suggest none");
    assert.ok(labels.includes("xs"), "Should suggest xs");
    assert.ok(labels.includes("sm"), "Should suggest sm");
    assert.ok(labels.includes("md"), "Should suggest md");
    assert.ok(labels.includes("lg"), "Should suggest lg");
    assert.ok(labels.includes("xl"), "Should suggest xl");
    assert.ok(labels.includes("2xl"), "Should suggest 2xl");
  });

  test("Suggests button variants inside <Button variant=\"", () => {
    const doc = createMockDoc(['<Button variant="']);
    const items = provider.provideCompletionItems(
      doc,
      new Position(0, 17),
      {} as any,
      {} as any
    ) as any[];

    const labels = items.map((i: any) => i.label);
    assert.ok(labels.includes("primary"), "Should suggest primary");
    assert.ok(labels.includes("secondary"), "Should suggest secondary");
    assert.ok(labels.includes("subtle"), "Should suggest subtle");
    assert.ok(labels.includes("danger"), "Should suggest danger");
  });

  test("Suggests badge statuses inside <Badge status=\"", () => {
    const doc = createMockDoc(['<Badge status="']);
    const items = provider.provideCompletionItems(
      doc,
      new Position(0, 15),
      {} as any,
      {} as any
    ) as any[];

    const labels = items.map((i: any) => i.label);
    assert.ok(labels.includes("info"), "Should suggest info");
    assert.ok(labels.includes("success"), "Should suggest success");
    assert.ok(labels.includes("warning"), "Should suggest warning");
    assert.ok(labels.includes("danger"), "Should suggest danger");
  });

  test("Suggests title orders inside <Title order=\"", () => {
    const doc = createMockDoc(['<Title order="']);
    const items = provider.provideCompletionItems(
      doc,
      new Position(0, 14),
      {} as any,
      {} as any
    ) as any[];

    const labels = items.map((i: any) => i.label);
    assert.ok(labels.includes("1"), "Should suggest 1");
    assert.ok(labels.includes("2"), "Should suggest 2");
    assert.ok(labels.includes("3"), "Should suggest 3");
  });
});

describe("ERM & EDS Hover Provider Tests", () => {
  const hoverProvider = new ErmHoverProvider();

  function createHoverDoc(line: string) {
    return {
      lineAt: () => ({ text: line }),
      getWordRangeAtPosition: (pos: any, regex: RegExp) => {
        let start = pos.character;
        let end = pos.character;
        while (start > 0 && regex.test(line[start - 1])) {
          start--;
        }
        while (end < line.length && regex.test(line[end])) {
          end++;
        }
        return new Range(new Position(0, start), new Position(0, end));
      },
      getText: (range: any) => line.substring(range.start.character, range.end.character),
    } as any;
  }

  test("Shows hover documentation for EDS Primitive (Button)", () => {
    const doc = createHoverDoc("<Button variant=\"primary\">");
    const hover = hoverProvider.provideHover(
      doc,
      new Position(0, 3), // hovering over 'Button'
      {} as any
    ) as any;

    assert.ok(hover, "Hover should not be null");
    assert.ok(hover.contents.value.includes("<Button>"), "Should document <Button>");
    assert.ok(
      hover.contents.value.includes("Eronom Design System"),
      "Should mention EDS"
    );
  });

  test("Shows hover documentation for EDS Token (canvas)", () => {
    const doc = createHoverDoc('<Center bg="canvas">');
    const hover = hoverProvider.provideHover(
      doc,
      new Position(0, 14), // hovering over 'canvas'
      {} as any
    ) as any;

    assert.ok(hover, "Hover should not be null");
    assert.ok(
      hover.contents.value.includes("canvas"),
      "Should document canvas token"
    );
    assert.ok(
      hover.contents.value.includes("light-dark"),
      "Should include dual-theme CSS output"
    );
  });
});

describe("ERM & EDS Auto-Close Tag Tests", () => {
  test("Detects open HTML tag <div> for auto-closing", () => {
    const tag = getAutoClosingTag("<div");
    assert.strictEqual(tag, "div");
  });

  test("Detects EDS Primitive <Box p=\"md\" bg=\"card\"> for auto-closing", () => {
    const tag = getAutoClosingTag('<Box p="md" bg="card"');
    assert.strictEqual(tag, "Box");
  });

  test("Detects EDS Primitive <Stack gap=\"md\"> for auto-closing", () => {
    const tag = getAutoClosingTag('<Stack gap="md"');
    assert.strictEqual(tag, "Stack");
  });

  test("Detects EDS Primitive <Button variant=\"primary\"> for auto-closing", () => {
    const tag = getAutoClosingTag('<Button variant="primary"');
    assert.strictEqual(tag, "Button");
  });

  test("Does NOT auto-close void HTML elements (img, input, br, hr)", () => {
    assert.strictEqual(getAutoClosingTag('<img src="avatar.png"'), null);
    assert.strictEqual(getAutoClosingTag('<input type="text"'), null);
    assert.strictEqual(getAutoClosingTag('<br'), null);
    assert.strictEqual(getAutoClosingTag('<hr'), null);
  });

  test("Does NOT auto-close self-closing tags", () => {
    assert.strictEqual(getAutoClosingTag('<Box p="md" /'), null);
    assert.strictEqual(getAutoClosingTag('<slot /'), null);
  });

  test("Does NOT auto-close closing tags", () => {
    assert.strictEqual(getAutoClosingTag('</div'), null);
    assert.strictEqual(getAutoClosingTag('</Box'), null);
  });

  test("Does NOT auto-close comments or doctypes", () => {
    assert.strictEqual(getAutoClosingTag('<!-- this is a comment'), null);
    assert.strictEqual(getAutoClosingTag('<!DOCTYPE html'), null);
  });

  test("Does NOT auto-close if '>' is inside attribute string or braces", () => {
    assert.strictEqual(getAutoClosingTag('<div title="x '), null);
    assert.strictEqual(getAutoClosingTag('<Button onClick={() => { x '), null);
  });

  test("findMatchingOpenTag finds matching open tag for '</' completion", () => {
    const mockDoc = {
      getText: () => "<Stack gap=\"md\">\n  <Card p=\"2xl\">\n    ",
    } as any;

    const matchingTag = findMatchingOpenTag(mockDoc, new Position(2, 4));
    assert.strictEqual(matchingTag, "Card", "Should find innermost unclosed Card");

    const mockDoc2 = {
      getText: () => "<Stack gap=\"md\">\n  <Card p=\"2xl\">\n  </Card>\n  ",
    } as any;
    const matchingTag2 = findMatchingOpenTag(mockDoc2, new Position(3, 2));
    assert.strictEqual(matchingTag2, "Stack", "Should find Stack when Card is already closed");
  });
});
