# Social cards

Images for Struo's posts on X ([@runstruo](https://x.com/runstruo)). Each
template has its own folder with a `card.html`, one JSON file per card, and an
`AGENTS.md` that covers when to use it and how to fill it in:

| Template | Use it for |
| --- | --- |
| [`release/`](release/AGENTS.md) | Announcing a Struo release and what changed since the last announced one |
| [`integration/`](integration/AGENTS.md) | Announcing a new infrastructure provider: the Struo mark and the provider's logo, joined |

## Rendering

```sh
socials/render.sh socials/release/2.17.2.json      # -> socials/out/release-2.17.2.png
socials/render.sh socials/integration/vercel.json  # -> socials/out/integration-vercel.png
```

Cards are 1000×562 CSS pixels rendered at 2x, so 2000×1124 PNGs, X's 16:9
image size. The script needs curl, jq, tar and `agent-browser` (preinstalled in
Amp orbs; `npm i -g agent-browser` elsewhere). It needs no Elixir setup.

Open the PNG and check it before showing it to anyone. Shorten text that wraps
or clips instead of changing the layout. Commit each card's JSON so it can be
rendered again; `out/` and `.cache/` are ignored.

## Design

The cards follow the layout of Tuist's release cards on
[@tuistdev](https://x.com/tuistdev/status/2123006567457165652), in Struo's look:

- Colors and radii come from Noora's `tokens.css` in dark mode. `render.sh`
  takes it from the Noora version pinned in `web/mix.lock`, so the cards follow
  the site when Noora is upgraded. Use `--noora-*` tokens, not new hex values.
- Struo's own touches: Geist Mono and the spiral mark from `web/priv/static/`,
  and the struo.run orange (`--struo-brand`) for accents only.
- A new kind of card gets a new folder with its own `card.html`, a sample JSON
  file and an `AGENTS.md`, plus a row in the table above. Keep it 1000×562
  and data-driven: `render.sh` injects the JSON as `window.CARD`.

## Posting

- Post text is lowercase and under 280 characters; X counts each link as 23.
- Every claim on the card or in the post must be backed by the release notes
  (`gh release view <tag> -R tuist/struo`) or
  [struo.run/changelog](https://struo.run/changelog).
- Nothing goes to X without Pedro's approval of both the text and the card.
  After he approves, he signs in to X as @runstruo in the browser himself (in an
  Amp orb, the Desktop tab). Never ask for the password in chat. Then attach the
  PNG, paste the text, and post.
