import { expect, test, describe } from "bun:test";
import * as fs from "fs";
import * as path from "path";

const extensionDir = path.resolve(__dirname, "..");

describe("Eronom Extension Configuration Tests", () => {
  const packageJsonPath = path.join(extensionDir, "package.json");

  test("package.json exists and is valid JSON", () => {
    expect(fs.existsSync(packageJsonPath)).toBe(true);
    const content = fs.readFileSync(packageJsonPath, "utf8");
    const pkg = JSON.parse(content);
    expect(pkg.name).toBe("eronom-extension");
    expect(pkg.main).toBe("./dist/extension.js");
  });

  test("All contributed files exist", () => {
    const content = fs.readFileSync(packageJsonPath, "utf8");
    const pkg = JSON.parse(content);

    // Check languages configurations
    const languages = pkg.contributes.languages;
    expect(languages).toBeArray();
    for (const lang of languages) {
      const configPath = path.resolve(extensionDir, lang.configuration);
      expect(fs.existsSync(configPath)).toBe(true);
      expect(() => JSON.parse(fs.readFileSync(configPath, "utf-8"))).not.toThrow();
    }

    // Check grammars
    const grammars = pkg.contributes.grammars;
    expect(grammars).toBeArray();
    for (const grammar of grammars) {
      const grammarPath = path.resolve(extensionDir, grammar.path);
      expect(fs.existsSync(grammarPath)).toBe(true);
      expect(() => JSON.parse(fs.readFileSync(grammarPath, "utf-8"))).not.toThrow();
    }

    // Check snippets
    const snippets = pkg.contributes.snippets;
    expect(snippets).toBeArray();
    for (const snippet of snippets) {
      const snippetPath = path.resolve(extensionDir, snippet.path);
      expect(fs.existsSync(snippetPath)).toBe(true);
      expect(() => JSON.parse(fs.readFileSync(snippetPath, "utf-8"))).not.toThrow();
    }
  });

  test("Compiled output exists", () => {
    const distPath = path.join(extensionDir, "dist", "extension.js");
    expect(fs.existsSync(distPath)).toBe(true);
  });
});
