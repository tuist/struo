defmodule StruoSiteWeb.Docs.OgImage do
  @moduledoc """
  Renders a documentation Open Graph card as a self-contained HTML page.

  The HTML is screenshotted to a JPG at build time by
  `Mix.Tasks.Docs.Gen.OgImages` via headless Chrome. Fonts and the logo are
  embedded as data URIs so the page renders with no network access.

  The card is dark and text only, in the brand colors: the logo and section sit
  at the top left, the category or authors at the top right, and the title is
  set in the lightest neutral under faint scanlines. The logo carries the orange.
  """
  use Phoenix.Component

  alias Phoenix.HTML.Safe

  @max_title_length 60
  @max_description_length 120
  @width 1920
  @height 1080

  def width, do: @width
  def height, do: @height

  attr :title, :string, required: true
  attr :description, :string, default: nil
  attr :category, :string, default: nil
  attr :subtitle, :string, default: "Docs"
  attr :avatars, :list, default: []
  attr :fonts, :map, required: true
  attr :logo_data_uri, :string, required: true

  def card(assigns) do
    ~H"""
    <html>
      <head>
        <meta charset="utf-8" />
        <style>
          @font-face {
            font-family: 'Geist Mono';
            font-style: normal;
            font-weight: 400 700;
            src: url(<%= @fonts.mono_ext %>) format('woff2');
            unicode-range: U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, U+0304, U+0308, U+0329, U+1D00-1DBF, U+1E00-1E9F, U+1EF2-1EFF, U+2020, U+20A0-20AB, U+20AD-20C0, U+2113, U+2C60-2C7F, U+A720-A7FF;
          }
          @font-face {
            font-family: 'Geist Mono';
            font-style: normal;
            font-weight: 400 700;
            src: url(<%= @fonts.mono %>) format('woff2');
            unicode-range: U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD;
          }
          @font-face {
            font-family: 'Geist Pixel Line';
            font-style: normal;
            font-weight: 500;
            src: url(<%= @fonts.display %>) format('woff2');
          }
          /* The card loads no stylesheet, so it restates the Noora colors it uses. */
          :root {
            --noora-neutral-dark-50: oklch(97.2% 0 0);
            --noora-neutral-dark-300: oklch(72% 0 0);
            --noora-neutral-dark-800: oklch(34.8% 0 0);
            --noora-neutral-dark-1200: oklch(16.2% 0 0);
          }
          * { margin: 0; padding: 0; box-sizing: border-box; }
          html, body {
            width: <%= @image_width %>px;
            height: <%= @image_height %>px;
            overflow: hidden;
            font-family: 'Geist Mono', monospace;
            color-scheme: dark;
            color: var(--noora-neutral-dark-50);
            background: var(--noora-neutral-dark-1200);
            -webkit-font-smoothing: antialiased;
          }
          body {
            position: relative;
            padding: 104px 112px;
            display: flex;
            flex-direction: column;
            gap: 152px;
          }
          /* Scanlines over the whole card. */
          body::before {
            position: absolute;
            inset: 0;
            z-index: 1;
            background: repeating-linear-gradient(to bottom, transparent 0 3px, oklch(0% 0 0 / 0.16) 3px 6px);
            content: "";
          }
          [data-part="header"], [data-part="brand"], [data-part="author-meta"] {
            display: flex;
            align-items: center;
            gap: 28px;
          }
          [data-part="header"] { min-width: 0; justify-content: space-between; gap: 64px; }
          [data-part="brand"] { flex-shrink: 0; }
          [data-part="logo"] { width: 88px; height: 88px; object-fit: contain; }
          [data-part="wordmark"], [data-part="subtitle"], [data-part="category"] {
            line-height: 88px;
            white-space: nowrap;
          }
          [data-part="wordmark"], [data-part="subtitle"] { font-size: 60px; letter-spacing: -1.8px; }
          [data-part="wordmark"] { font-weight: 600; }
          [data-part="subtitle"] { font-weight: 400; }
          [data-part="divider"] { width: 2px; height: 72px; background: var(--noora-neutral-dark-800); }
          [data-part="author-meta"] { min-width: 0; justify-content: flex-end; }
          [data-part="category"] {
            min-width: 0;
            font-size: 44px;
            font-weight: 400;
            letter-spacing: -1.3px;
            overflow: hidden;
            text-overflow: ellipsis;
          }
          [data-part="author-avatars"] {
            display: flex;
            flex-shrink: 0;
            flex-direction: row-reverse;
          }
          [data-part="author-avatar"] {
            width: 72px;
            height: 72px;
            margin-right: -16px;
            border: 4px solid var(--noora-neutral-dark-1200);
            border-radius: 50%;
            background: var(--noora-neutral-dark-800);
            object-fit: cover;
          }
          [data-part="author-avatar"]:last-child { margin-right: 0; }
          [data-part="content"] {
            min-height: 0;
            display: flex;
            flex-direction: column;
            gap: 48px;
          }
          [data-part="title"], [data-part="description"] {
            flex-shrink: 0;
            display: -webkit-box;
            -webkit-box-orient: vertical;
            -webkit-line-clamp: 3;
            overflow: hidden;
            overflow-wrap: anywhere;
          }
          [data-part="title"] {
            font-family: 'Geist Pixel Line', 'Geist Mono', monospace;
            font-weight: 500;
            line-height: 1.08;
            color: var(--noora-neutral-dark-50);
            text-wrap: balance;
          }
          [data-part="title"][data-size="large"] { font-size: 168px; letter-spacing: -3px; }
          [data-part="title"][data-size="medium"] { font-size: 128px; letter-spacing: -2px; }
          [data-part="title"][data-size="small"] { font-size: 96px; letter-spacing: -1px; }
          [data-part="description"] {
            font-size: 52px;
            font-weight: 400;
            letter-spacing: -1.5px;
            line-height: 1.4;
            color: var(--noora-neutral-dark-300);
          }
        </style>
      </head>
      <body>
        <header data-part="header">
          <div data-part="brand">
            <img data-part="logo" src={@logo_data_uri} />
            <div data-part="wordmark">Struo</div>
            <div :if={@subtitle} data-part="divider"></div>
            <div :if={@subtitle} data-part="subtitle">{@subtitle}</div>
          </div>
          <div :if={@avatars != [] || @category} data-part="author-meta">
            <div :if={@avatars != []} data-part="author-avatars">
              <img :for={avatar <- @avatars} data-part="author-avatar" src={avatar} />
            </div>
            <div :if={@category} data-part="category">{@category}</div>
          </div>
        </header>
        <main data-part="content">
          <div data-part="title" data-size={title_size(@title)}>
            {truncate(@title, @max_title_length)}
          </div>
          <div :if={@description} data-part="description">
            {summarize(@description, @max_description_length)}
          </div>
        </main>
      </body>
    </html>
    """
  end

  @doc "Render the full OG card HTML for a page."
  def render_html(opts) do
    fonts_dir = Keyword.fetch!(opts, :fonts_dir)
    logo_path = Keyword.fetch!(opts, :logo_path)

    assigns = %{
      title: Keyword.fetch!(opts, :title),
      description: Keyword.get(opts, :description),
      category: Keyword.get(opts, :category),
      subtitle: Keyword.get(opts, :subtitle, "Docs"),
      avatars: Keyword.get(opts, :avatars, []),
      fonts: %{
        mono: font_data_uri(fonts_dir, "GeistMono-latin.woff2"),
        mono_ext: font_data_uri(fonts_dir, "GeistMono-latin-ext.woff2"),
        display: font_data_uri(fonts_dir, "GeistPixel-Line.woff2")
      },
      logo_data_uri: data_uri(MIME.from_path(logo_path), logo_path),
      image_width: @width,
      image_height: @height,
      max_title_length: @max_title_length,
      max_description_length: @max_description_length
    }

    "<!DOCTYPE html>" <>
      (assigns |> card() |> Safe.to_iodata() |> IO.iodata_to_binary())
  end

  defp font_data_uri(fonts_dir, file), do: data_uri("font/woff2", Path.join(fonts_dir, file))

  defp data_uri(mime, path), do: "data:#{mime};base64,#{path |> File.read!() |> Base.encode64()}"

  # Short titles are set large; long ones step down so three lines still fit.
  # A word longer than a line would break in the middle, so it steps down too.
  defp title_size(title) do
    title = truncate(title, @max_title_length)
    longest_word = title |> String.split() |> Enum.map(&String.length/1) |> Enum.max(fn -> 0 end)

    case {String.length(title), longest_word} do
      {length, word} when length <= 26 and word <= 17 -> "large"
      {length, word} when length <= 42 and word <= 23 -> "medium"
      _ -> "small"
    end
  end

  @doc "Map a docs slug to its OG image filename, e.g. `guide/why` -> `guide-why.jpg`."
  def slug_to_filename(segments) when is_list(segments) do
    case segments do
      [] -> "index.jpg"
      _ -> Enum.join(segments, "-") <> ".jpg"
    end
  end

  @doc """
  Shortens text to `max` characters: whole sentences when the first ones fit,
  so it rarely ends mid-thought, and otherwise whole words and an ellipsis.
  """
  def summarize(text, max) do
    if String.length(text) > max do
      case Regex.run(~r/^.{39,}[.!?](?=\s)/s, String.slice(text, 0, max + 1)) do
        [sentences] -> sentences
        nil -> truncate(text, max)
      end
    else
      text
    end
  end

  defp truncate(nil, _max), do: ""

  defp truncate(text, max) do
    if String.length(text) > max do
      text
      |> String.slice(0, max + 1)
      |> String.replace(~r/\s+\S*$/, "")
      |> String.slice(0, max)
      |> String.replace(~r/[\s,;:.]+$/, "")
      |> Kernel.<>("...")
    else
      text
    end
  end
end
