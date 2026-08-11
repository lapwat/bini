# bini

Bini is a package manager that installs binaries on your system.

# Usage

```sh
Usage: bini [OPTIONS] [NAME] [COMMAND]

Commands:
  install  Install from GitHub repository
  list     List installed binaries
  update   Update installed binaries
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

The index of installed binaries is stored in `~/.local/state/bini/index.txt`. It keeps track of what you have installed, and with which name.

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
- [ ] remove command
- [ ] install from GitLab
- [ ] install from Codeberg
