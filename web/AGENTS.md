This is a Phoenix application that serves the Struo marketing site and the documentation (under `/docs`).

## CSS And Styling

- Use `data-part` attributes as CSS selectors instead of classes.
- Keep cards at 8px radius or less.
- Do not add a new styling framework unless the project needs it.
- The docs and marketing surfaces both build on the Noora design system and its tokens.

## Infrastructure Providers

- Ship each new provider with its guide and an entry in the Infrastructure Providers sidebar: Tuist first, then the other providers in alphabetical order. Follow the guide's Capabilities, Prerequisites, Configuration, Usage, and Limits and cleanup structure; distinguish execution from cache support and unverified live behavior.
- Give a provider's sidebar entry no icon, and do not vendor its official logo or favicon. Sidebar icons are single-color Noora or Tabler outline icons, and a provider gets one only when an icon in that style has been designed for it.

## Changelog

- When you ship a substantive user-facing change, add an entry under `priv/changelog/`
  as a markdown file named `YYYY-MM-DD-short-slug.md` with `title` and `date`
  frontmatter. Entries appear on `/changelog` and in the RSS/Atom feeds.
- Small visual and aesthetic refinements do not need a changelog entry.

## Running

Use the repository toolchain through mise:

```sh
mise exec -- mix setup
mise exec -- mix phx.server
mise exec -- mix precommit
```
