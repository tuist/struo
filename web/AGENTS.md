This is a Phoenix application that serves the Struo marketing site and the documentation (under `/docs`).

## CSS And Styling

- Use `data-part` attributes as CSS selectors instead of classes.
- Keep cards at 8px radius or less.
- Do not add a new styling framework unless the project needs it.
- The docs and marketing surfaces both build on the Noora design system and its tokens.

## Infrastructure Providers

- Ship each new provider with its guide and an alphabetized entry in the Infrastructure Providers sidebar, including an icon. Follow the guide's Capabilities, Prerequisites, Configuration, Usage, and Limits and cleanup structure; distinguish execution from cache support and unverified live behavior.
- Vendor the provider's official logo or favicon under `priv/static/docs/icons/` and register its CSS in `assets/docs/docs.css`. Use a theme-aware mask for monochrome transparent marks and a background image for full-color or opaque assets. Record the official source URL in the change description.
- Keep the sidebar and rendered-guide tests requiring an icon for every provider. Verify that the local asset loads and inspect the selected entry in light and dark themes, including narrow navigation; do not defer the logo or weaken the checks to ship a guide.

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
