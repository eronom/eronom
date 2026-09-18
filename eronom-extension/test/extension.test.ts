import { test, describe } from "node:test";
import assert from "node:assert";
import * as fs from "fs";
import * as path from "path";

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

  test("ERM grammar top-level-script does not swallow markup on line 1", () => {
    const ermGrammarPath = path.join(extensionDir, "syntaxes", "erm.tmLanguage.json");
    const ermGrammar = JSON.parse(fs.readFileSync(ermGrammarPath, "utf8"));
    const topLevelScript = ermGrammar.repository["top-level-script"];
    assert.ok(topLevelScript, "top-level-script pattern should exist");
    assert.notStrictEqual(
      topLevelScript.begin,
      "\\A",
      "top-level-script should not unconditionally match \\A without lookahead"
    );
    assert.ok(
      topLevelScript.begin.includes("export") || topLevelScript.begin.includes("(?="),
      "top-level-script should verify script keywords before starting"
    );
  });

  test("ERM grammar includes fragment pattern and return in control-blocks", () => {
    const ermGrammarPath = path.join(extensionDir, "syntaxes", "erm.tmLanguage.json");
    const ermGrammar = JSON.parse(fs.readFileSync(ermGrammarPath, "utf8"));
    assert.ok(ermGrammar.repository["fragment"], "fragment pattern should exist");
    assert.ok(
      ermGrammar.repository["control-blocks"].patterns[0].match.includes("return"),
      "control-blocks should match return keyword"
    );
  });
});
