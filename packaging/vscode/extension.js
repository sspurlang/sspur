// The SSPUR extension: starts `sspur lsp` and adds the Run, Test and Fill holes commands.
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client;

function binary() {
  const p = vscode.workspace.getConfiguration("sspur").get("path");
  return p && p.trim() ? p.trim() : "sspur";
}

async function start() {
  const command = binary();
  const server = { command, args: ["lsp"] };
  client = new LanguageClient("sspur", "SSPUR", { run: server, debug: server }, {
    documentSelector: [
      { scheme: "file", language: "sspur" },
      { scheme: "untitled", language: "sspur" },
    ],
  });
  try {
    await client.start();
  } catch (e) {
    client = undefined;
    const pick = await vscode.window.showErrorMessage(
      `Couldn't start '${command} lsp' (${e.message || e}). Install SSPUR or set sspur.path.`,
      "Open settings",
      "Install SSPUR",
    );
    if (pick === "Open settings") vscode.commands.executeCommand("workbench.action.openSettings", "sspur.path");
    if (pick === "Install SSPUR") vscode.env.openExternal(vscode.Uri.parse("https://github.com/utkarshavardhana/sspur#install"));
  }
}

async function stop() {
  if (client) await client.stop();
  client = undefined;
}

// Saves the file and runs `sspur run` or `sspur test` on it in a terminal.
async function runFile(sub) {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== "sspur") {
    vscode.window.showWarningMessage("Open an SSPUR file first.");
    return;
  }
  const doc = editor.document;
  if (doc.isUntitled) {
    vscode.window.showWarningMessage("Save the file first.");
    return;
  }
  if (doc.isDirty) await doc.save();
  const folder = vscode.workspace.getWorkspaceFolder(doc.uri);
  const exec = new vscode.ProcessExecution(binary(), [sub, doc.uri.fsPath], { cwd: folder ? folder.uri.fsPath : require("path").dirname(doc.uri.fsPath) });
  const task = new vscode.Task({ type: "sspur", command: sub }, folder || vscode.TaskScope.Workspace, `${sub} ${require("path").basename(doc.uri.fsPath)}`, "sspur", exec, []);
  task.presentationOptions = { reveal: vscode.TaskRevealKind.Always, clear: true, focus: false };
  await vscode.tasks.executeTask(task);
}

// Asks the server for the best candidate of each typed hole and applies them to the buffer.
async function fillHoles() {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== "sspur" || !client) return;
  const uri = editor.document.uri.toString();
  await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: "Filling holes" }, async () => {
    const r = await client.sendRequest("workspace/executeCommand", { command: "sspur.fill", arguments: [uri] });
    const n = (r && r.filled) || 0;
    vscode.window.setStatusBarMessage(n ? `SSPUR: filled ${n} hole${n === 1 ? "" : "s"}` : "SSPUR: no hole has a passing candidate", 5000);
  });
}

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("sspur.run", () => runFile("run")),
    vscode.commands.registerCommand("sspur.test", () => runFile("test")),
    vscode.commands.registerCommand("sspur.fillHoles", fillHoles),
    vscode.commands.registerCommand("sspur.restart", async () => {
      await stop();
      await start();
    }),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("sspur.path")) {
        await stop();
        await start();
      }
    }),
  );
  await start();
}

function deactivate() {
  return stop();
}

module.exports = { activate, deactivate };
