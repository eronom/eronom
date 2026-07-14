import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';

export function activate(context: vscode.ExtensionContext) {
    console.log('Eronom support extension is active.');

    // Register simple commands (build, dev, start)
    const simpleCommands = [
        { id: 'eronom.build', cmd: 'build' },
        { id: 'eronom.dev', cmd: 'dev' },
        { id: 'eronom.start', cmd: 'start' }
    ];

    simpleCommands.forEach(command => {
        let disposable = vscode.commands.registerCommand(command.id, async () => {
            const workspaceFolders = vscode.workspace.workspaceFolders;
            if (!workspaceFolders) {
                vscode.window.showErrorMessage('Please open a workspace before running Eronom commands.');
                return;
            }

            const cwd = workspaceFolders[0].uri.fsPath;
            const binaryName = process.platform === 'win32' ? 'eronom.exe' : 'eronom';
            const localBinaryPath = path.join(cwd, binaryName);
            
            let commandToRun: string;
            if (fs.existsSync(localBinaryPath)) {
                commandToRun = `./${binaryName} ${command.cmd}`;
            } else {
                commandToRun = `eronom ${command.cmd}`;
            }
            
            let terminal = vscode.window.terminals.find(t => t.name === 'Eronom');
            if (!terminal) {
                terminal = vscode.window.createTerminal({
                    name: 'Eronom',
                    cwd: cwd
                });
            }
            terminal.show();
            terminal.sendText(commandToRun);
        });

        context.subscriptions.push(disposable);
    });

    // Register custom interactive init command
    let initDisposable = vscode.commands.registerCommand('eronom.init', async () => {
        const workspaceFolders = vscode.workspace.workspaceFolders;
        if (!workspaceFolders) {
            vscode.window.showErrorMessage('Please open a workspace before running Eronom commands.');
            return;
        }

        const cwd = workspaceFolders[0].uri.fsPath;
        
        // Ask user for a folder/project name
        const folderName = await vscode.window.showInputBox({
            prompt: 'Enter folder name to initialize Eronom project (leave empty to use the current directory)',
            placeHolder: 'my-eronom-app'
        });
        
        // If they cancel (undefined), do nothing
        if (folderName === undefined) {
            return;
        }

        let targetDir = cwd;
        if (folderName.trim() !== '') {
            targetDir = path.join(cwd, folderName.trim());
        }

        let forceFlag = '';
        if (fs.existsSync(targetDir)) {
            try {
                const files = fs.readdirSync(targetDir);
                if (files.length > 0) {
                    const choice = await vscode.window.showWarningMessage(
                        `The directory "${folderName.trim() || '.'}" is not empty. Initialize anyway?`,
                        'Yes (Force)',
                        'No'
                    );
                    if (choice !== 'Yes (Force)') {
                        return;
                    }
                    forceFlag = ' --force';
                }
            } catch (err) {
                // Ignore error and proceed without force
            }
        }

        const binaryName = process.platform === 'win32' ? 'eronom.exe' : 'eronom';
        const localBinaryPath = path.join(cwd, binaryName);
        
        let commandToRun: string;
        if (fs.existsSync(localBinaryPath)) {
            commandToRun = `./${binaryName} init`;
        } else {
            commandToRun = `eronom init`;
        }

        if (folderName.trim() !== '') {
            commandToRun += ` "${folderName.trim()}"`;
        }
        
        if (forceFlag) {
            commandToRun += forceFlag;
        }
        
        let terminal = vscode.window.terminals.find(t => t.name === 'Eronom');
        if (!terminal) {
            terminal = vscode.window.createTerminal({
                name: 'Eronom',
                cwd: cwd
            });
        }
        terminal.show();
        terminal.sendText(commandToRun);
    });

    context.subscriptions.push(initDisposable);
}

export function deactivate() {}
