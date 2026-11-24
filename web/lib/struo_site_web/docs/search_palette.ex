defmodule StruoSiteWeb.Docs.SearchPalette do
  @moduledoc """
  The documentation's search menu: a dialog with a search input and the
  results for what is typed in it, from `StruoSiteWeb.Docs.Search`.

  The `DocsSearch` hook opens and closes the dialog and moves the selection
  with the keyboard; the results are rendered here.
  """
  use Phoenix.LiveComponent
  use Noora

  alias Phoenix.LiveView.JS
  alias StruoSiteWeb.Docs.Search

  @groups [page: "Pages", section: "Sections", content: "Content"]

  @impl true
  def mount(socket) do
    {:ok, assign(socket, query: "", results: Search.suggestions())}
  end

  @impl true
  def handle_event("search", %{"q" => query}, socket) do
    query = query |> String.slice(0, Search.max_query_length()) |> String.trim()
    results = if query == "", do: Search.suggestions(), else: Search.search(query)

    {:noreply, assign(socket, query: query, results: results)}
  end

  @impl true
  def render(assigns) do
    assigns = assign(assigns, :groups, groups(assigns.query, assigns.results))

    ~H"""
    <div id={@id} phx-hook="DocsSearch">
      <dialog
        id="docs-search-dialog"
        aria-label="Search the documentation"
        phx-mounted={JS.ignore_attributes(["open"])}
      >
        <form
          id="docs-search-form"
          data-part="search-row"
          phx-change="search"
          phx-submit="search"
          phx-target={@myself}
        >
          <.text_input
            id="docs-search-input"
            type="search"
            name="q"
            value={@query}
            placeholder="Search the docs"
            autocomplete="off"
            phx-debounce="100"
            role="combobox"
            aria-expanded="true"
            aria-autocomplete="list"
            aria-controls="docs-search-results"
            aria-label="Search the documentation"
          >
            <:suffix>
              <.shortcut_key size="small">esc</.shortcut_key>
            </:suffix>
          </.text_input>
        </form>
        <div id="docs-search-results" role="listbox" aria-label="Search results">
          <div :for={{label, results} <- @groups} role="group" aria-label={label} data-part="group">
            <p data-part="group-label" aria-hidden="true">{label}</p>
            <.link
              :for={{result, index} <- results}
              id={"docs-search-result-#{index}"}
              patch={result.url}
              role="option"
              aria-selected="false"
              data-part="result"
            >
              <span data-part="result-icon"><.icon name={icon_name(result.kind)} /></span>
              <span data-part="result-text">
                <span data-part="result-heading">
                  <span data-part="result-title">{marked(result.title)}</span>
                  <span data-part="result-trail">{result.trail}</span>
                </span>
                <span :if={result.snippet} data-part="result-snippet">
                  {marked(result.snippet)}
                </span>
              </span>
              <span data-part="result-enter" aria-hidden="true">
                <%!-- Tabler corner-down-left, which Noora does not ship. --%>
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.5"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                >
                  <path d="M18 6v6a3 3 0 0 1 -3 3h-10l4 -4m0 8l-4 -4" />
                </svg>
              </span>
            </.link>
          </div>
        </div>
        <div :if={@results == []} data-part="empty">
          <.table_empty_state
            icon="search"
            title={"No results for “#{@query}”"}
            subtitle="Try fewer words, or a different one."
          />
        </div>
        <footer>
          <span><.shortcut_key size="small">↑</.shortcut_key><.shortcut_key size="small">
            ↓
          </.shortcut_key>
          Navigate</span>
          <span><.shortcut_key size="small">↵</.shortcut_key>
          Open</span>
          <span><.shortcut_key size="small">esc</.shortcut_key>
          Close</span>
        </footer>
      </dialog>
    </div>
    """
  end

  # Marks the matched parts of a title or an excerpt. Built as one run of
  # text, since whitespace between the parts would show as spaces.
  defp marked(segments) do
    for {text, highlighted?} <- segments do
      escaped = Phoenix.HTML.html_escape(text)
      if highlighted?, do: [{:safe, "<mark>"}, escaped, {:safe, "</mark>"}], else: escaped
    end
  end

  # Results by kind, each numbered by its place in the whole list, which is
  # the order the keyboard walks them in.
  defp groups("", results), do: [{"Suggested", Enum.with_index(results)}]

  defp groups(_query, results) do
    indexed = Enum.with_index(results)

    for {kind, label} <- @groups,
        group = Enum.filter(indexed, fn {result, _index} -> result.kind == kind end),
        group != [] do
      {label, group}
    end
  end

  defp icon_name(:page), do: "file_text"
  defp icon_name(:section), do: "list_tree"
  defp icon_name(:content), do: "menu_2"
end
