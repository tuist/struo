# Release card

Announces a Struo release with the changes since the last announced version:
the version as the title, three highlights and the install command. The
highlights cover everything since the last announced version, not only the
latest release. Rendering, design and approval rules are in
[`../AGENTS.md`](../AGENTS.md).

## When to announce

Struo publishes a GitHub release for nearly every merge, often several a day, so
not every release gets a post. Post when a few user-facing features have built
up since the last announced version. Releases that only change documentation,
CI or internals don't count. A release that adds an infrastructure provider can
also get its own card in [`../integration/`](../integration/AGENTS.md).

## 1. Collect what changed

```sh
# Releases published after the last announced one.
gh release list -R tuist/struo -L 200 --json tagName,publishedAt \
  -q '.[] | select(.publishedAt > "<last announced release date>") | .tagName'
gh release view <tag> -R tuist/struo
```

Keep the `✨ Features` entries, and `⚡ Performance` or `🐛 Bug Fixes` only
when users would notice them. Take the wording from the matching entries at
[struo.run/changelog](https://struo.run/changelog), which explain them in user
terms. Don't use counts, provider names or capabilities the notes don't support.

## 2. Fill in the card

Copy the latest `<version>.json` to `<new version>.json` and edit it:

| Field | Value |
| --- | --- |
| `version` | the latest release; shown in the title and the install line |
| `date` | the latest release's date, e.g. `November 24, 2026` |
| `category` | `CLI` for the CLI; `SDK` or `Site` when the release is only about those |
| `highlights` | exactly three `[title, description]` pairs; titles of two or three words, descriptions of about 40 characters or fewer so they fit on one line |

Then run `socials/render.sh socials/release/<version>.json`.

## 3. Write the post

```text
struo <version> is out
- <what users can do now>
- <what users can do now>
- <what users can do now>

https://struo.run/changelog
```

Bullets say what users get, not how it is built. Mention `@tuistdev` when a
feature involves Tuist.
