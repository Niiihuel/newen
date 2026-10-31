# limn integration

quill builds against `../limn-quill-integration`, a worktree of limn on the branch
`quill/integration`: `main` plus the pull requests quill needs that are not merged yet.
When they are all merged, point `Cargo.toml` back at `../lienzo` and drop the branch.

| PR | Branch | What quill uses it for | State |
|---|---|---|---|
| [#120](https://github.com/Niiihuel/limn/pull/120) | `feat/split-view` | chat \| diff and diff / terminal panes | open |
| [#121](https://github.com/Niiihuel/limn/pull/121) | `feat/code-diff` | word highlights, folded context, file headers in the changes panel | open |
| [#122](https://github.com/Niiihuel/limn/pull/122) | `feat/terminal` | the terminal panel | open |
| [#123](https://github.com/Niiihuel/limn/pull/123) | `feat/split-button` | "Always allow ▾" on the permission card | open |
| [#124](https://github.com/Niiihuel/limn/pull/124) | `feat/status-marks` | pulsing session dots, the context ring | open |
| [#128](https://github.com/Niiihuel/limn/pull/128) | `feat/spinner-dots` | the running line's spinner | open |
| [#129](https://github.com/Niiihuel/limn/pull/129) | `feat/choice-fill` | the accent of the chosen radio on question cards | open |
| [#130](https://github.com/Niiihuel/limn/pull/130) | `feat/stream-fade` | the reply fading in as it streams | open |
| — | `feat/rolling-number` | the branch bar's +N −M rolling to each new count | local, not pushed |
| — | `fix/submenu-gap` | the composer's submenus | local, not pushed |
| — | `feat/modal-flush-body` | Settings' modal, a body without padding inside a whole outline, colored apart (`outline`) | local, not pushed |
| — | `feat/breadcrumb-fade` | the Files pane's path: `Breadcrumb::fade_overflow` keeps every segment and fades the end that does not fit instead of collapsing into "…" | local, not pushed |
| — | `feat/select-plain` | Settings' plain selects | local, not pushed |
| — | `feat/switch-off-ink` | Settings' off switches on Desktop's pale gray track | local, not pushed |
| — | `feat/segmented-icons` | Settings' theme picker: 26 px icon segments and the chosen one outlined | local, not pushed |
| — | `feat/action-row-line-height` | Settings' descriptions with their lines 20 px apart | local, not pushed |
| — | `feat/focus-area` | the browser pane's input surface (`Element::focus_area` public) | local, not pushed |
| [#132](https://github.com/Niiihuel/limn/pull/132) | `feat/code-editor-host-look` | quill-ide's editor in quill's code inks (types and properties too), and its own mode in the footer (on #73) | open |
| [#133](https://github.com/Niiihuel/limn/pull/133) | `feat/context-menu-sizing` | context menus at the measures of quill's menus, in quill and quill-ide | open |
| [#134](https://github.com/Niiihuel/limn/pull/134) | `feat/code-editor-change-marks` | quill-ide's git marks in the editor's gutter | open |
| [#135](https://github.com/Niiihuel/limn/pull/135) | `fix/code-editor-line-width` | the editor's horizontal scroll without flicker | open |
| [#137](https://github.com/Niiihuel/limn/pull/137) | `feat/code-editor-scrollbar` | quill-ide's editor scrollbar in quill's rail with its arrows | open |
| [#138](https://github.com/Niiihuel/limn/pull/138) | `feat/menu-picture` | the installed editors' own icons in quill's "Open in" menu | open |
| — | `feat/menu-switch` | switch items in menus (`DropdownMenuItem::switch`): the composer's Connectors, each with its switch | local, not pushed |
| — | `feat/menu-switch-ink` | a menu switch that is on takes the menu's `check_ink`; builds on `feat/menu-switch` and `feat/desktop-menus` | local, not pushed |
| — | `feat/submenu-width` | a submenu with its own width (`DropdownMenuItem::submenu_width`): the session menu's "Transcript view" | local, not pushed |
| — | `feat/suggestion-compact` | one-line suggestion rows, a wider cap, the selected row's description beside it, matched text in bold, the host's text sizes: the composer's "/" palette | local, not pushed |
| — | `feat/viewer-morph` | `ImageViewer` actions beside close, a morph from the thumbnail (`Easing::CubicBezier`) and a lightbox mode (the picture fitted, its caption under it): the composer's images | local, not pushed |
| — | `feat/sketch` | `Sketch`, marks drawn over an image and painted into its pixels: annotating an attached image | local, not pushed |
| — | `fix/modal-focus-tooltip` | a popover's opening focus and its backdrop raise no tooltip: the image viewer's close showed "Close image · Esc" all the time | local, not pushed |
| — | `fix/list-numbers` | a numbered list keeps "10." on one line and its items' text in line (a two-digit column, the number at its end) | local, not pushed |
| — | `feat/scribble` | `Scribble`, a working indicator that writes and fades; quill tried its own pen with `marks` and `pen` and went back to shimmering words, so quill uses none of it now | local, not pushed |
| — | `feat/surface-image` | the browser's native frames handed to egui as they are (`SurfaceFrame::from_image`) | local, not pushed |
| — | `feat/surface-texture` | the browser's GPU frames drawn from a texture quill registered with egui-wgpu (`present_surface_texture`) | local, not pushed |
| — | `feat/surface-paint` | the page presented beneath the window: quill paints the browser's surface itself, a transparent hole (`present_surface_paint`) | local, not pushed |
| — | `feat/drawn-over` | no longer used: native tabs now paint into their surface, so nothing has to step aside for quill (docs/browser.md §8) | local, not pushed; can be dropped |
| — | `feat/textarea-persistent-placeholder` | the composer's placeholder, and the agent's suggested prompt in it, kept while the box is focused and empty (`Composer::persistent_placeholder`) | local, not pushed |

Updating the branch: `git -C ../limn-quill-integration merge <branch>`; the only conflicts so
far were rows added side by side in `docs/components.md`, `CHANGELOG.md` and `lib.rs`, and
`site/src/data/pages.json`, regenerated with `cargo run -p limn-gallery -- --pages`.
