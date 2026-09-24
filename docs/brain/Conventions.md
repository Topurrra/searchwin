---
tags: [searchwin, conventions]
updated: 2026-09-24
---

# Conventions

Back to [[README]]

## Boundaries

- **Never modify `C:\Users\user\Projects\Search`** (the Mac app). It's read-only
  reference. The only exception is one line in its local `.git/info/exclude`
  that ignores `searchwin/`.
- All work happens in `searchwin\`, which is its own git repo.
- Testing never uses the real profile, never types passwords into forms, and
  never imports the user's real browser data. See [[Testing]].

## Git

- Branch: `main`, remote `origin` = https://github.com/Topurrra/searchwin
- Identity for commits:
  `git -c user.name=topurrra -c user.email=topurianika96@gmail.com commit …`
- Commit subjects say what changed for the user, e.g. *"Fix View menu crash in release builds"*.
  Skip the body unless it's truly needed.
- Commits made by Claude never end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, never write this into commit messages.
- Commit messages are short, as short as it can be. 

## Code style

- Match the Mac app: the same names, the same file split, the same behaviour.
- Comments are **prose that explains why**, written in the app's voice
  (`/// The tabs, down the left instead of across the top.`). No boilerplate
  XML doc tags.
- UI is built in C#. Colours come from `Palette` (light/dark pairs). Icons come
  from `Icons` (Segoe Fluent glyphs).
- AOT-safe by default:
  - classes that touch WinRT are `partial`
  - WinRT casts use `.As<T>()`, not C# casts or `is`
  - JSON goes through the `Json` source-generated context
  - no reflection
- Errors that shouldn't crash the app go to `Links.Trouble(e)`.
- UI changes must be marshalled to `UI.Queue`.

## After each session

Update this vault:
- [[Changelog]]
- [[Lessons Learned]], for anything that cost more than ten minutes
- [[Decisions]], for any real choice
- a `Log/YYYY-MM-DD` note
