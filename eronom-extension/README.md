# Eronom VS Code Extension

Language support for the Eronom web framework, including syntax highlighting, snippets, and commands for `.er` (Eronom scripting) and `.erm` (Eronom components) files.

## Features

- **Syntax Highlighting**:
  - Full syntax highlighting for `.er` scripting files, identifying keywords like `fn`, `struct`, `embed`, `interface`, and variables.
  - Multi-language syntax highlighting for `.erm` component files, seamlessly embedding HTML syntax, CSS in `<style>` blocks, JS/ER in `<script>` blocks, and bracket expressions/loops in HTML templates.
- **Auto-Closing & Formatting**:
  - Code completions, bracket matches, and comment toggling.
- **Snippets**:
  - Code snippets for common structures like reactive states (`useState`), control structures, blocks, functions, and struct definitions.
- **Commands**:
  - Command palette integrations for starting development/production servers and building projects.

## Commands Available

- `Eronom: Initialize Project` - Runs `eronom init`.
- `Eronom: Build Project` - Runs `eronom build`.
- `Eronom: Start Development Server` - Runs `eronom dev`.
- `Eronom: Start Production Server` - Runs `eronom start`.
