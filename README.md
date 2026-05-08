# FLgit

FLgit is a Windows version control companion for FL Studio. It provides an overlay UI for managing FL Studio project versions through Git, Git LFS, and GitHub CLI, and semantic diffs for `.flp` files through flpdiff integration.

## Features

- **FL Studio overlay:** Collapsed launcher and expanded source-control panel designed to sit over FL Studio
- **Project binding:** Bind an existing `.flp` file or FL Studio project folder
- **Collaboration cloning:** Clone a GitHub repo, pull Git LFS assets, and bind the cloned project automatically
- **Git project setup:** Initialize local repos with FLgit defaults, Git LFS tracking, and `Backup/` ignores
- **Source control actions:** Stage, unstage, commit, pull, push, reset, and manage origin remotes
- **GitHub publishing:** Create and push new GitHub repos through GitHub CLI
- **Semantic FLP diffs:** View meaningful `.flp` changes through flpdiff, plus surface summaries for other files
- **History tools:** Inspect commits, compare two commits, and view changed project files
- **Conflict handling:** Detect merge/rebase states and resolve binary file conflicts by choosing local or remote
- **Edit locks:** Commit a `.flgit/lock.json` lock to signal active editing
- **Project watching:** Refresh status when FL Studio saves project changes

## Development

#### Dependencies:

- Rust and Cargo
- Node.js and npm
- flpdiff (`src-tauri/bin/flpdiff-windows-x64.exe`)

FLgit bundles `flpdiff` for local semantic FLP diff support, grab it [here](https://github.com/dawhubapp/flpdiff/releases) and place it at `src-tauri/bin/flpdiff-windows-x64.exe`

#### Commands

```powershell
# install frontend dependencies
npm install

# build the frontend
npm run build

# run frontend tests
node --test

# run Rust tests
cd src-tauri
cargo test

# return to repo root, then run the Tauri app
cd ..
npm run tauri dev
```

## Installation

#### Dependencies:

- Windows
- Git
- Git LFS
- GitHub CLI
- FL Studio

#### Portable setup:

1. Build a FLgit portable release folder
2. Install Git, Git LFS, and GitHub CLI, if not already installed
    1. Open PowerShell and configure Git with your name and email:
        1. `git config --global user.name "Your Name"`
        2. `git config --global user.email "you@example.com"`
    2. Run Git LFS setup from PowerShell:
        1. `git lfs install`
    3. Log in to GitHub CLI from PowerShell:
        1. `gh auth login`
5. Launch `FLgit.exe`
6. Use the `Bind Project` button for an existing local FL Studio project, or `Clone Project` for a GitHub collaboration repo
    1. Hover over buttons to learn what they do!

FLgit stores local app preferences in your Windows app config folder. Closing FLgit clears project binding, so the next launch starts ready for a new bind or clone.

## Credits

Semantic FLP parsing and diffing is powered by [flpdiff](https://github.com/dawhubapp/flpdiff), created by the goat [pronskiy](https://github.com/pronskiy)

## MIT License

This project is licensed under the MIT License.