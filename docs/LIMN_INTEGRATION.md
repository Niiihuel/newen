# limn integration

Newen builds against `../limn-quill-integration`, a worktree of limn on the branch
`quill/integration`. That branch equals limn's `main`, except while Newen waits on a limn pull
request not merged yet: then it is `main` plus that pull request, listed below until it merges.

Every branch Newen used to carry on it (split view, code diff, terminal, menus, the image
viewer's lightbox, host-fed surfaces and the rest) landed in limn's `main` with
[#168](https://github.com/Niiihuel/limn/pull/168) and
[#169](https://github.com/Niiihuel/limn/pull/169), which also moved limn to egui/eframe 0.37;
Newen follows it on the same versions.

| PR | Branch | What Newen uses it for | State |
|---|---|---|---|
| [#180](https://github.com/Niiihuel/limn/pull/180) | `feat/code-look` | newen-ide's look: a diff in the editor's syntax inks, fold chevrons shown over the gutter | open |
| [#181](https://github.com/Niiihuel/limn/pull/181) | `feat/rust-1.99` | Rust 1.99.1, the toolchain Newen builds with | open |

Keeping it so:

- After limn's `main` moves: `git -C ../limn-quill-integration fetch origin` and
  `git -C ../limn-quill-integration merge origin/main`, then push `quill/integration`.
- When Newen needs a limn PR before it merges: `git -C ../limn-quill-integration merge
  origin/<branch>`, push, and add its row above; drop the row once it is in `main`.
- Never force-push `quill/integration` or rebase it: CI and the packaging jobs check it out
  by name (`.github/workflows`), and other worktrees build on it.

The only conflicts so far were rows added side by side in `docs/components.md`, `CHANGELOG.md`
and `lib.rs`, and `site/src/data/pages.json`, regenerated with
`cargo run -p limn-gallery -- --pages`.
