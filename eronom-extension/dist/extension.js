"use strict";
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// src/extension.ts
var extension_exports = {};
__export(extension_exports, {
  activate: () => activate,
  deactivate: () => deactivate
});
module.exports = __toCommonJS(extension_exports);
var vscode = __toESM(require("vscode"));
var path = __toESM(require("path"));
var fs = __toESM(require("fs"));
function activate(context) {
  console.log("Eronom support extension is active.");
  const simpleCommands = [
    { id: "eronom.build", cmd: "build" },
    { id: "eronom.dev", cmd: "dev" },
    { id: "eronom.start", cmd: "start" }
  ];
  simpleCommands.forEach((command) => {
    let disposable = vscode.commands.registerCommand(command.id, async () => {
      const workspaceFolders = vscode.workspace.workspaceFolders;
      if (!workspaceFolders) {
        vscode.window.showErrorMessage("Please open a workspace before running Eronom commands.");
        return;
      }
      const cwd = workspaceFolders[0].uri.fsPath;
      const binaryName = process.platform === "win32" ? "eronom.exe" : "eronom";
      const localBinaryPath = path.join(cwd, binaryName);
      let commandToRun;
      if (fs.existsSync(localBinaryPath)) {
        commandToRun = `./${binaryName} ${command.cmd}`;
      } else {
        commandToRun = `eronom ${command.cmd}`;
      }
      let terminal = vscode.window.terminals.find((t) => t.name === "Eronom");
      if (!terminal) {
        terminal = vscode.window.createTerminal({
          name: "Eronom",
          cwd
        });
      }
      terminal.show();
      terminal.sendText(commandToRun);
    });
    context.subscriptions.push(disposable);
  });
  let initDisposable = vscode.commands.registerCommand("eronom.init", async () => {
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders) {
      vscode.window.showErrorMessage("Please open a workspace before running Eronom commands.");
      return;
    }
    const cwd = workspaceFolders[0].uri.fsPath;
    const folderName = await vscode.window.showInputBox({
      prompt: "Enter folder name to initialize Eronom project (leave empty to use the current directory)",
      placeHolder: "my-eronom-app"
    });
    if (folderName === void 0) {
      return;
    }
    let targetDir = cwd;
    if (folderName.trim() !== "") {
      targetDir = path.join(cwd, folderName.trim());
    }
    let forceFlag = "";
    if (fs.existsSync(targetDir)) {
      try {
        const files = fs.readdirSync(targetDir);
        if (files.length > 0) {
          const choice = await vscode.window.showWarningMessage(
            `The directory "${folderName.trim() || "."}" is not empty. Initialize anyway?`,
            "Yes (Force)",
            "No"
          );
          if (choice !== "Yes (Force)") {
            return;
          }
          forceFlag = " --force";
        }
      } catch (err) {
      }
    }
    const binaryName = process.platform === "win32" ? "eronom.exe" : "eronom";
    const localBinaryPath = path.join(cwd, binaryName);
    let commandToRun;
    if (fs.existsSync(localBinaryPath)) {
      commandToRun = `./${binaryName} init`;
    } else {
      commandToRun = `eronom init`;
    }
    if (folderName.trim() !== "") {
      commandToRun += ` "${folderName.trim()}"`;
    }
    if (forceFlag) {
      commandToRun += forceFlag;
    }
    let terminal = vscode.window.terminals.find((t) => t.name === "Eronom");
    if (!terminal) {
      terminal = vscode.window.createTerminal({
        name: "Eronom",
        cwd
      });
    }
    terminal.show();
    terminal.sendText(commandToRun);
  });
  context.subscriptions.push(initDisposable);
}
function deactivate() {
}
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
  activate,
  deactivate
});
//# sourceMappingURL=extension.js.map
