defmodule StruoSiteWeb.Docs.Markdown do
  @moduledoc """
  Renders documentation markdown into the HTML the Noora docs styles expect,
  plus a title, a one-paragraph description, an on-this-page outline, and the
  raw source (for "copy page").

  Pipeline: strip frontmatter, expand VitePress containers
  (`StruoSiteWeb.Docs.Containers`), render with MDEx, then post-process the HTML
  with `StruoSiteWeb.Docs.HTML` (alerts, code windows, tables, heading anchors)
  and rewrite internal links under `/docs`.
  """

  alias StruoSiteWeb.Docs.Containers
  alias StruoSiteWeb.Docs.HTML

  @type heading :: %{level: pos_integer(), id: String.t(), text: String.t()}
  @type t :: %{
          title: String.t() | nil,
          description: String.t() | nil,
          html: String.t(),
          headings: [heading()],
          markdown: String.t()
        }

  # Tokens are marked with classes, and the site's stylesheet colors them.
  # Syntax highlighting is disabled in tests for speed and determinism.
  @syntax_highlight (if Mix.env() == :test do
                       [syntax_highlight: nil]
                     else
                       [syntax_highlight: [engine: :lumis, opts: [formatter: :html_linked]]]
                     end)

  @mdex_options [
                  extension: [
                    header_id_prefix: "",
                    table: true,
                    strikethrough: true,
                    tasklist: true,
                    autolink: true,
                    alerts: true
                  ],
                  render: [unsafe: true]
                ] ++ @syntax_highlight

  @doc "Render a markdown document into `%{title:, description:, html:, headings:, markdown:}`."
  @spec render(String.t()) :: t()
  def render(markdown) do
    source = strip_frontmatter(markdown)

    rendered =
      source
      |> Containers.preprocess()
      |> MDEx.to_html!(@mdex_options)

    # Read-only parse for the title and outline; the output stays a string so
    # syntax-highlighting whitespace is never collapsed by Floki.
    tree = Floki.parse_fragment!(rendered)
    title = extract_title(tree)
    headings = extract_headings(tree)

    html =
      rendered
      |> HTML.build_alerts()
      |> HTML.wrap_code_blocks()
      |> HTML.add_heading_anchors()
      |> HTML.rewrite_links()
      |> HTML.wrap_tables()

    %{
      title: title,
      description: summary(source),
      html: html,
      headings: headings,
      markdown: source
    }
  end

  @doc """
  The first prose paragraph after the title, as plain text, or `nil` when a
  page opens with something else. It describes the page in its head and on its
  Open Graph card.
  """
  @spec summary(String.t()) :: String.t() | nil
  def summary(markdown) do
    markdown
    |> strip_frontmatter()
    |> String.replace(~r/^#.*$/m, "")
    |> String.split("\n\n", trim: true)
    |> Enum.map(&String.trim/1)
    |> Enum.find(fn block ->
      block != "" and not String.starts_with?(block, ["```", "|", ":::", "<", "- ", "* "])
    end)
    |> case do
      nil ->
        nil

      block ->
        block
        |> String.replace(~r/\[([^\]]*)\]\([^)]*\)/, "\\1")
        |> String.replace(~r/[`*>#\[\]()]|(?<!\w)_|_(?!\w)/, "")
        |> String.replace(~r/\s+/, " ")
        |> String.trim()
    end
  end

  defp strip_frontmatter("---\n" <> rest) do
    case String.split(rest, "\n---", parts: 2) do
      [_frontmatter, body] -> String.trim_leading(body, "\n")
      _ -> "---\n" <> rest
    end
  end

  defp strip_frontmatter(markdown), do: markdown

  defp extract_title(tree) do
    case Floki.find(tree, "h1") do
      [node | _] -> node |> Floki.text() |> String.trim()
      [] -> nil
    end
  end

  defp extract_headings(tree) do
    tree
    |> Floki.find("h2, h3, h4")
    |> Enum.map(fn {tag, attrs, _children} = node ->
      text = node |> Floki.text() |> String.trim()
      %{level: level(tag), id: attr(attrs, "id") || slugify(text), text: text}
    end)
  end

  defp level("h2"), do: 2
  defp level("h3"), do: 3
  defp level("h4"), do: 4
  defp level(_), do: 2

  defp attr(attrs, key) do
    Enum.find_value(attrs, fn
      {^key, value} -> value
      _ -> nil
    end)
  end

  defp slugify(text) do
    text
    |> String.downcase()
    |> String.replace(~r/[^a-z0-9]+/u, "-")
    |> String.trim("-")
  end
end
