# bini

Bini is a package manager that installs binaries from GitHub releases on your system.

Checksums are not checked. Trust your sources !

# Usage

```sh
Usage: bini [OPTIONS] [NAME] [COMMAND]

Commands:
  install  Install from GitHub repository [alias: i]
  list     List installed binaries [alias: l]
  update   Update all installed binaries [aliases: u, ]
  remove   Remove installed binary [aliases: r, rm, uninstall]
  help     Print this message or the help of the given subcommand(s)

Arguments:
  [NAME]  The name of the package to install

Options:
      --as <AS_NAME>  Install the binary under a different name
  -h, --help          Print help
  -V, --version       Print version

Examples:
  bini install sharkdp/bat
  bini install burntsushi/ripgrep --as rg
  bini i sharkdp/bat
  bini sharkdp/bat
```

# Update strategy

Update are handled by checking the date of a release against the modification date of your local binary.

# Storage

The index of installed binaries is stored in `~/.local/state/bini/index.txt`. It keeps track of what you have installed, and under what name.

Binaries are stored in `~/.local/share/bini/bin/` folder. You may add this folder to your $PATH.

**For bash**

```sh
echo 'export PATH="$HOME/.local/share/bini/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

**For zsh**

```sh
echo 'export PATH="$HOME/.local/share/bini/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

# Todo

- [x] install command
- [x] list command
- [x] update command
- [x] remove command
- [ ] install from GitLab
- [ ] install from Codeberg
- [ ] take host os and architecture into account
