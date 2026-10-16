use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

struct Sspur;

impl zed::Extension for Sspur {
    fn new() -> Self {
        Sspur
    }

    fn language_server_command(&mut self, id: &LanguageServerId, worktree: &zed::Worktree) -> Result<zed::Command> {
        let binary = LspSettings::for_worktree(id.as_ref(), worktree).ok().and_then(|s| s.binary);
        let configured = binary.as_ref().and_then(|b| b.path.clone());
        let command = match configured.or_else(|| worktree.which("sspur")) {
            Some(path) => path,
            None => {
                return Err("sspur was not found on PATH. Install it (https://github.com/sspurlang/sspur#install), \
                    or set lsp.sspur.binary.path in Zed's settings, then restart the language server."
                    .into());
            }
        };
        let args = binary.as_ref().and_then(|b| b.arguments.clone()).unwrap_or_else(|| vec!["lsp".into()]);
        let mut env = worktree.shell_env();
        if let Some(extra) = binary.and_then(|b| b.env) {
            env.extend(extra);
        }
        Ok(zed::Command { command, args, env })
    }
}

zed::register_extension!(Sspur);
