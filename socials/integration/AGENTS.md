# Infrastructure provider card

Announces that Struo integrates with a new infrastructure provider, a remote
execution sandbox or a cache backend. The card holds only the Struo mark and the
provider's logo in two tiles joined in the middle, and `struo × <provider>`
below them, under the same scanlines as the docs and blog OG images. What the
integration does goes in the post, not on the card. Rendering, design and approval rules are in
[`../AGENTS.md`](../AGENTS.md).

**On hold:** we are not posting provider announcements yet. Keep the cards
ready, adding one for each new provider, but post one only when Pedro says to.

## 1. Gather the facts

The provider's entry at [struo.run/changelog](https://struo.run/changelog) and
its guide under `struo.run/docs/guide/infrastructure/` are the sources: the
`kind`, the release that added it, and what it supports. If the changelog states
a limitation, such as "live provider validation remains pending" or "does not
supply shared caching", the post says so too.

## 2. Point to the logo

`logo` is a URL that `render.sh` downloads into `../.cache/`. Don't commit
provider logos to this repository (see `web/AGENTS.md`).

- Prefer the provider's press kit or brand page, and follow its usage rules.
  For single-color marks, [Simple Icons](https://simpleicons.org) (CC0) works:
  `https://cdn.jsdelivr.net/npm/simple-icons@14/icons/<slug>.svg`.
- Use the symbol, not a wordmark, so it fills the square tile.
- Set `logo_mode` to `mono` for single-color marks; the card paints them white.
  Use `color` for multi-color logos, which render as they are, and check them
  against the dark tile.

## 3. Fill in the card

Copy `vercel.json` to `<provider>.json` and edit it:

| Field | Value |
| --- | --- |
| `provider` | the product name as the provider writes it, e.g. `Vercel Sandbox`; keep `struo × <provider>` on one line |
| `logo`, `logo_mode` | see above |

Then run `socials/render.sh socials/integration/<provider>.json`.

## 4. Write the post

Tag the provider's X account when it has one:

```text
struo now runs your scripts in @<provider> sandboxes
- <what users get>
- <what users get>

https://struo.run/docs/guide/infrastructure/<provider>
```
