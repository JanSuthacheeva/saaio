use clap::CommandFactory;
use clap_complete::Shell;

use crate::cli::Cli;
use crate::cli::output::Document;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The shell: bash, zsh, fish, elvish or powershell
    shell: Shell,
}

/// `saaio completions <shell>`: the shell's completion script for every command saaio --help
/// lists. Runs without a store.
pub fn run(args: &Args) -> Document {
    let mut script = Vec::new();
    clap_complete::generate(args.shell, &mut listed(), "saaio", &mut script);
    // println! adds the line break the script ends in.
    Document::Text(String::from_utf8_lossy(&script).trim_end().to_string())
}

/// Cli's command without the commands saaio --help hides, which clap_complete would offer.
fn listed() -> clap::Command {
    let cli = Cli::command();
    let visible = cli
        .get_subcommands()
        .filter(|command| !command.is_hide_set())
        .cloned();
    clap::Command::new("saaio")
        .version(crate::proto::BUILD)
        .args(cli.get_arguments().cloned())
        .subcommands(visible)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_commands_saaio_help_lists() {
        let names: Vec<_> = listed()
            .get_subcommands()
            .map(|command| command.get_name().to_string())
            .collect();

        assert_eq!(
            names,
            [
                "new",
                "report",
                "status",
                "start",
                "remove",
                "send",
                "clear",
                "bug-report",
                "completions"
            ]
        );
    }
}
