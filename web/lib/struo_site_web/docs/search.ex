defmodule StruoSiteWeb.Docs.Search do
  @moduledoc """
  Full-text search over the documentation.

  Every markdown page under the docs root is split into sections: the text
  before its first heading, then one section per `h2` to `h4`. A query matches
  a section when each of its words appears in the page title, the section
  heading, or the section text. Matches on the title rank first, then matches
  on a heading, then matches in the text, which carry an excerpt.

  The sections are built from the rendered pages once and kept until a
  markdown file changes.
  """

  alias StruoSiteWeb.Docs
  alias StruoSiteWeb.Docs.Cache
  alias StruoSiteWeb.Docs.Sidebar

  defmodule Result do
    @moduledoc false
    defstruct [:kind, :url, :title, :trail, :snippet]

    @type segment :: {String.t(), highlighted? :: boolean()}
    @type t :: %__MODULE__{
            kind: :page | :section | :content,
            url: String.t(),
            title: [segment()],
            trail: String.t(),
            snippet: [segment()] | nil
          }
  end

  @index_key {__MODULE__, :index}
  @max_query_length 100
  @max_terms 8
  @limits %{page: 5, section: 6, content: 6}
  @snippet_lead 48
  @snippet_length 170
  @skipped_tags ~w(svg button script style template)
  @skipped_parts ~w(bar hash tabs)
  @heading_tags ~w(h2 h3 h4)
  @inline_tags ~w(a abbr b code del em i kbd mark s span strong sub sup)

  @doc "The longest query `search/1` reads; the rest is dropped."
  def max_query_length, do: @max_query_length

  @doc "Results for `query`, best first within pages, then sections, then content."
  @spec search(String.t()) :: [Result.t()]
  def search(query) when is_binary(query) do
    case terms(query) do
      [] -> []
      terms -> sections() |> Enum.flat_map(&match(&1, terms)) |> best()
    end
  end

  # The best matches of each kind, pages first.
  defp best(matches) do
    by_kind =
      matches
      |> Enum.sort_by(fn {score, position, _result} -> {-score, position} end)
      |> Enum.map(fn {_score, _position, result} -> result end)
      |> Enum.group_by(& &1.kind)

    Enum.flat_map([:page, :section, :content], fn kind ->
      by_kind |> Map.get(kind, []) |> Enum.take(@limits[kind])
    end)
  end

  @doc "Pages offered before anything is typed: the guide's first group."
  @spec suggestions() :: [Result.t()]
  def suggestions do
    [group | _] = Sidebar.guide_tree()

    for item <- Sidebar.pages(group) do
      %Result{kind: :page, url: item.slug, title: [{item.label, false}], trail: "Guide"}
    end
  end

  @doc "Build the sections ahead of the first search."
  def warm do
    sections()
    :ok
  end

  defp terms(query) do
    query
    |> String.slice(0, @max_query_length)
    |> String.downcase()
    |> String.split()
    |> Enum.uniq()
    |> Enum.take(@max_terms)
  end

  # Matching

  defp match(section, terms) do
    in_title = Enum.filter(terms, &String.contains?(section.title_folded, &1))
    in_heading = Enum.filter(terms, &String.contains?(section.heading_folded, &1))
    in_text = Enum.filter(terms, &String.contains?(section.text_folded, &1))

    cond do
      terms -- (in_title ++ in_heading ++ in_text) != [] ->
        []

      section.heading == nil and terms -- in_title == [] ->
        [{300 + title_bonus(section, terms), section.position, page_result(section, terms)}]

      in_heading != [] and terms -- (in_heading ++ in_title) == [] ->
        score = 200 + 10 * length(in_heading)
        [{score, section.position, section_result(section, terms)}]

      in_text != [] ->
        score = 100 + 10 * length(in_title ++ in_heading)
        [{score, section.position, content_result(section, terms)}]

      true ->
        []
    end
  end

  defp title_bonus(section, terms) do
    query = Enum.join(terms, " ")

    cond do
      section.title_folded == query -> 100
      String.starts_with?(section.title_folded, query) -> 50
      true -> 0
    end
  end

  defp page_result(section, terms) do
    %Result{
      kind: :page,
      url: section.url,
      title: highlight(section.title, terms),
      trail: section.trail
    }
  end

  defp section_result(section, terms) do
    %Result{
      kind: :section,
      url: section.url,
      title: highlight(section.heading, terms),
      trail: trail(section)
    }
  end

  defp content_result(section, terms) do
    %Result{
      kind: :content,
      url: section.url,
      title: highlight(section.heading || section.title, terms),
      trail: trail(section),
      snippet: snippet(section, terms)
    }
  end

  defp trail(%{heading: nil, trail: trail}), do: trail
  defp trail(section), do: section.trail <> " › " <> section.title

  # An excerpt of the text around its first match, cut at spaces.
  defp snippet(section, terms) do
    text = section.text

    if byte_size(text) == byte_size(section.text_folded) do
      {at, _length} = :binary.match(section.text_folded, terms)
      size = byte_size(text)
      from = if at <= @snippet_lead, do: 0, else: after_space(text, at - @snippet_lead, at)

      to =
        if from + @snippet_length >= size,
          do: size,
          else: next_space(text, from + @snippet_length)

      prefix = if from > 0, do: "… ", else: ""
      suffix = if to < size, do: " …", else: ""

      [{prefix, false}] ++
        highlight(binary_part(text, from, to - from), terms) ++ [{suffix, false}]
    else
      [{String.slice(text, 0, @snippet_length), false}]
    end
  end

  defp after_space(text, from, limit) do
    case :binary.match(text, " ", scope: {from, limit - from}) do
      {at, 1} -> at + 1
      :nomatch -> limit
    end
  end

  defp next_space(text, from) do
    case :binary.match(text, " ", scope: {from, byte_size(text) - from}) do
      {at, 1} -> at
      :nomatch -> byte_size(text)
    end
  end

  # Splits `text` into segments, marking each occurrence of a term. Offsets
  # come from the lowercased text, so they are only used when lowercasing
  # kept every byte in place.
  defp highlight(text, terms) do
    folded = String.downcase(text)

    if byte_size(folded) == byte_size(text) do
      {segments, rest_from} =
        folded
        |> :binary.matches(terms)
        |> Enum.reduce({[], 0}, fn {at, length}, {segments, from} ->
          {[
             {binary_part(text, at, length), true},
             {binary_part(text, from, at - from), false} | segments
           ], at + length}
        end)

      [{binary_part(text, rest_from, byte_size(text) - rest_from), false} | segments]
      |> Enum.reverse()
      |> Enum.reject(fn {segment, _highlighted?} -> segment == "" end)
    else
      [{text, false}]
    end
  end

  # Sections

  defp sections do
    files = [Docs.root(), "**", "*.md"] |> Path.join() |> Path.wildcard() |> Enum.sort()
    stamp = :erlang.phash2(Enum.map(files, &{&1, mtime(&1)}))

    case :persistent_term.get(@index_key, nil) do
      {^stamp, sections} ->
        sections

      _ ->
        sections = build(files)
        :persistent_term.put(@index_key, {stamp, sections})
        sections
    end
  end

  defp mtime(file) do
    case File.stat(file, time: :posix) do
      {:ok, %File.Stat{mtime: mtime}} -> mtime
      _ -> 0
    end
  end

  defp build(files) do
    sidebar = sidebar_pages()
    trails = sidebar |> Enum.reverse() |> Map.new()
    order = sidebar |> Enum.with_index() |> Map.new(fn {{url, _trail}, index} -> {url, index} end)

    files
    |> Enum.flat_map(fn file ->
      url = url(file)
      page = Cache.get(file)
      title = page.title || url

      page
      |> page_sections()
      |> Enum.map(fn {heading, text} ->
        %{
          url: if(heading, do: url <> "#" <> heading.id, else: url),
          title: title,
          heading: heading && heading.text,
          trail: Map.get_lazy(trails, url, fn -> tab_label(url) end),
          text: text,
          title_folded: String.downcase(title),
          heading_folded: String.downcase((heading && heading.text) || ""),
          text_folded: String.downcase(text)
        }
      end)
    end)
    |> Enum.sort_by(&Map.get(order, page_url(&1.url), map_size(order)))
    |> Enum.with_index(fn section, position -> Map.put(section, :position, position) end)
  end

  defp page_url(url), do: url |> String.split("#") |> hd()

  defp url(file) do
    path = file |> Path.relative_to(Docs.root()) |> Path.rootname()

    Path.split(path)
    |> Enum.reverse()
    |> case do
      ["index"] -> "/docs"
      ["index" | parents] -> "/docs/" <> (parents |> Enum.reverse() |> Enum.join("/"))
      _ -> "/docs/" <> path
    end
  end

  # The text of a rendered page, split at its headings. Controls and labels
  # drawn around code are left out.
  defp page_sections(page) do
    headings = Map.new(page.headings, &{&1.id, &1})

    page.html
    |> Floki.parse_fragment!()
    |> collect(headings, [{nil, []}])
    |> Enum.reverse()
    |> Enum.map(fn {heading, text} ->
      {heading,
       text |> Enum.reverse() |> Enum.join() |> String.replace(~r/\s+/u, " ") |> String.trim()}
    end)
  end

  defp collect(nodes, headings, sections) when is_list(nodes) do
    Enum.reduce(nodes, sections, &collect(&1, headings, &2))
  end

  defp collect(text, _headings, [{heading, texts} | sections]) when is_binary(text) do
    [{heading, [text | texts]} | sections]
  end

  defp collect({"h1", _attrs, _children}, _headings, sections), do: sections

  defp collect({tag, attrs, children}, headings, sections) when tag in @heading_tags do
    case Map.fetch(headings, attribute(attrs, "id")) do
      {:ok, heading} -> [{heading, []} | sections]
      :error -> collect(children, headings, sections)
    end
  end

  defp collect({tag, attrs, children}, headings, sections) do
    cond do
      tag in @skipped_tags or attribute(attrs, "data-part") in @skipped_parts -> sections
      tag in @inline_tags -> collect(children, headings, sections)
      true -> collect([" " | children] ++ [" "], headings, sections)
    end
  end

  defp collect(_other, _headings, sections), do: sections

  defp attribute(attrs, name) do
    Enum.find_value(attrs, fn
      {^name, value} -> value
      _ -> nil
    end)
  end

  # Each sidebar page with where it sits, such as "Guide › Toolchains", in
  # reading order.
  defp sidebar_pages do
    for {tab, tree} <- [{"Guide", Sidebar.guide_tree()}, {"Reference", Sidebar.reference_tree()}],
        group <- tree,
        {url, parents} <- group_pages(group) do
      {url, Enum.join([tab | parents], " › ")}
    end
  end

  defp group_pages(group) do
    own = if is_binary(group.slug), do: [{group.slug, []}], else: []
    own ++ item_pages(group.items, [group.label])
  end

  defp item_pages(items, parents) do
    Enum.flat_map(items, fn item ->
      own = if is_binary(item.slug), do: [{item.slug, parents}], else: []
      own ++ item_pages(item.items, parents ++ [item.label])
    end)
  end

  defp tab_label(url) do
    case Sidebar.tab_for_slug(url) do
      :reference -> "Reference"
      _ -> "Guide"
    end
  end
end
