# Every control and its label

75 controls on 10 screens, each with a label and a test id. Written by `src/__tests__/accessibility.test.tsx`.

| Screen | Control | Label | Test id |
| --- | --- | --- | --- |
| Pair with your Mac | field | This phone's name | `pair.name` |
| Pair with your Mac | field | Code | `pair.code` |
| Pair with your Mac | button | Pair | `pair.submit` |
| Agents | button | Search Agents | `agents.search` |
| Agents | button | Menu | `agents.menu` |
| Agents | button | All | `agents.filter.all` |
| Agents | button | Active | `agents.filter.active` |
| Agents | button | Needs you, 0 | `agents.filter.needs` |
| Agents | button | shop, 1 agent | `agents.repo.shop` |
| Agents | button | Refresh sessions and changelog, done, claude, claude (existing login) | `agents.row.r-e230dd1eaaa5` |
| Agents | button | Archive, Refresh sessions and changelog | `agents.row.r-e230dd1eaaa5.archive` |
| Agents | button | New agent | `agents.new` |
| Conversation | button | Back | `agent.back` |
| Conversation | button | Changes | `agent.changes` |
| Conversation | button | More | `agent.more` |
| Conversation | button | CHANGELOG.md, Open at the edited hunk in the review | `agent.row.edit:30.file.0` |
| Conversation | button | Created CHANGELOG.md, +3 −0 | `agent.row.tool:r-e230dd1eaaa5_toolu_changelog` |
| Conversation | button | session-refresh-coordinator.ts, Open at the edited hunk in the review | `agent.row.edit:18.file.0` |
| Conversation | button | README.md, Open at the edited hunk in the review | `agent.row.edit:22.file.0` |
| Conversation | button | Copy code | `agent.row.msg:29.md.4.copy` |
| Conversation | button | /fixture/home/worktrees/shop-21f518a0/refresh-sessions-and-changelog/src/auth/session-refresh-coordinator.ts | `agent.row.msg:29.md.5.1` |
| Conversation | link | refresh token spec | `agent.row.msg:29.md.5.3` |
| Conversation | button | 6 steps, Read, Searched, Found files, Created, Edited, Ran | `agent.row.steps:9` |
| Conversation | field | Message to this agent | `agent.composer.text` |
| Conversation | button | Model: Default | `agent.composer.model` |
| Conversation | button | Effort: Default | `agent.composer.effort` |
| Conversation | button | Permissions: Default | `agent.composer.mode` |
| Conversation | button | Attach a photo or an image | `agent.composer.attach` |
| Conversation | button | Send | `agent.composer.send` |
| Changes | button | Back | `changes.back` |
| Changes | button | Fold src | `changes.folder.src/` |
| Changes | button | src/cart.ts, M, +2 −1, 0 of 2 reviewed | `changes.row.src/cart.ts` |
| Changes | button | src/tax.ts, A, New file, +2 −1, 0 of 2 reviewed | `changes.row.src/tax.ts` |
| Changes | field | Filter files | `changes.filter` |
| Changes | button | Comparison: Latest run | `changes.comparison` |
| A file's changes | button | Back | `file.back` |
| A file's changes | button | Scroll long lines sideways | `file.wrap` |
| A file's changes | button | Accept hunk 1 | `file.hunk.accept` |
| A file's changes | button | Reject hunk 1 | `file.hunk.reject` |
| A file's changes | button | Accept hunk 2 | `file.hunk.accept` |
| A file's changes | button | Reject hunk 2 | `file.hunk.reject` |
| A file's changes | button | Accept hunk 2 | `file.hunk.accept` |
| A file's changes | button | Reject hunk 2 | `file.hunk.reject` |
| Merge back | button | Back | `merge.back` |
| Merge back | button | 2 files, overseer/fix-cart → main | `merge.lands` |
| Merge back | button | cart.ts, M, src/cart.ts | `merge.lands.src/cart.ts` |
| Merge back | button | tax.ts, A, src/tax.ts | `merge.lands.src/tax.ts` |
| Merge back | button | Complete | `merge.complete` |
| Pull request | button | Back | `pr.back` |
| Pull request | field | Title | `pr.field.title` |
| Pull request | field | Description | `pr.field.body` |
| Pull request | button | Open. Pushes overseer/fix-cart to origin and opens a pull request on owner/shop. Nothing is merged. | `pr.open` |
| New agent | button | Back | `new.back` |
| New agent | field | Task | `new.task` |
| New agent | button | Repository, shop | `new.repo` |
| New agent | button | Agent, Claude Code | `new.agent` |
| New agent | button | Account, claude (existing login) | `new.account` |
| New agent | button | Model, Default | `new.model` |
| New agent | button | Effort, Default | `new.effort` |
| New agent | button | Permissions, Default | `new.mode` |
| New agent | button | Workspace, New worktree | `new.where` |
| New agent | button | Start | `new.start` |
| Accounts | button | Back | `accounts.back` |
| Accounts | button | Sign in, Work | `accounts.signin.p-work` |
| Settings | button | Back | `settings.back` |
| Settings | button | Allow notifications | `settings.notifications.allow` |
| Settings | switch | Notifications | `settings.notifications.all` |
| Settings | switch | Permission requests | `settings.notifications.permission` |
| Settings | switch | Questions | `settings.notifications.question` |
| Settings | switch | Errors | `settings.notifications.failure` |
| Settings | switch | Finished | `settings.notifications.finished` |
| Settings | switch | Show text in notifications | `settings.notifications.text` |
| Settings | switch | App lock | `settings.safety.lock` |
| Settings | switch | Ask for unlock before changes that cannot be undone | `settings.safety.unlock` |
| Settings | button | Forget this Mac | `settings.forget` |
