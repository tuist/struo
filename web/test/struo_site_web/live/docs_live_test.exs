defmodule StruoSiteWeb.DocsLiveTest do
  use StruoSiteWeb.ConnCase

  import Phoenix.LiveViewTest

  test "GET /docs renders the landing page", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/docs")

    assert html =~ "id=\"docs-layout\""
    assert html =~ ~s(data-part="feature-card")
    assert html =~ ~s(property="og:image:width" content="1920")
    assert html =~ ~s(property="og:image:height" content="1080")
    assert html =~ ~s(name="twitter:image:alt" content="Documentation")
  end

  test "renders a guide page with the docs layout and prose", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/docs/guide/why")

    assert html =~ "id=\"docs-sidebar\""
    assert html =~ ~s(<a href="/" data-phx-link=)
    assert html =~ ~s(data-part="logo-link")
    assert html =~ "id=\"docs-mobile-menu-trigger\""
    assert html =~ "aria-controls=\"docs-sidebar\""
    assert html =~ "id=\"docs-mobile-sidebar-overlay\""
    assert html =~ "data-prose"
  end

  test "searches the documentation from the search menu", %{conn: conn} do
    {:ok, view, html} = live(conn, ~p"/docs/guide/why")

    assert html =~ ~s(id="docs-search-trigger")
    assert has_element?(view, ~s(#docs-search-dialog [role="option"]), "Getting Started")

    results =
      view
      |> element("#docs-search-form")
      |> render_change(%{"q" => "getting started"})

    assert results =~ ~s(href="/docs/guide/getting-started")

    assert has_element?(
             view,
             ~s(#docs-search-result-0[href="/docs/guide/getting-started"] mark),
             "Getting"
           )

    view
    |> element("#docs-search-form")
    |> render_change(%{"q" => "zzzqq"})

    refute has_element?(view, ~s(#docs-search-dialog [role="option"]))
    assert has_element?(view, ~s(#docs-search-dialog [data-part="empty"]), "zzzqq")
  end

  test "renders the native Cargo adoption path", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/docs/guide/graph/rust")

    assert html =~ "Start from an existing Cargo project"
    assert html =~ "Choose how much configuration to own"
    assert html =~ "struo query targets --kind rust_binary"
  end

  test "renders a hand-written reference page", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/docs/reference/prelude/apple_library")

    assert html =~ "data-prose"
    assert html =~ "apple_library"
  end

  test "serves infrastructure provider guides and contracts", %{conn: conn} do
    providers =
      StruoSiteWeb.Docs.Sidebar.guide_tree()
      |> Enum.find(&(&1.label == "Infrastructure Providers"))

    for provider <- providers.items do
      {:ok, view, _html} = live(conn, provider.slug)
      assert has_element?(view, "article h1")
      assert has_element?(view, "article strong", "Capabilities:")

      for heading <- ["Prerequisites", "Configuration", "Usage", "Limits and cleanup"] do
        assert has_element?(view, "article h2", heading)
      end

      assert has_element?(view, "#docs-sidebar a[href='#{provider.slug}'][data-selected]")
    end

    for {path, title} <- [
          {"/docs/guide/infrastructure/providers", "Become an Infrastructure Provider"},
          {"/docs/guide/infrastructure/boat", "Boat"},
          {"/docs/guide/infrastructure/runloop", "Runloop Devboxes"},
          {"/docs/reference/infrastructure", "Infrastructure Provider Contracts"},
          {"/docs/reference/infrastructure/cache", "Cache Provider Contract"},
          {"/docs/reference/infrastructure/execution", "Execution Provider Contract"}
        ] do
      {:ok, view, _html} = live(conn, path)
      assert has_element?(view, "article h1", title)
    end
  end

  test "documents explicit modules, dependency checks, and deferred planning", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/docs/guide/graph/apple")
    assert html =~ "Explicit modules and dependency checks"
    assert html =~ "dependency_check"
    assert html =~ "not a downloadable hermetic Apple toolchain"

    {:ok, _view, html} = live(conn, ~p"/docs/reference/modules")
    assert html =~ "Deferred Action Planning"
    assert html =~ "expand_actions"
    assert html =~ "host_tree_sha256"
  end

  test "directory pages link to their actual editable source", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/docs/guide/graph")

    assert has_element?(
             view,
             "a[href='https://github.com/tuist/struo/edit/main/web/priv/docs/guide/graph/index.md']"
           )
  end

  test "generated reference pages do not offer edits to unpublished markdown", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/docs/reference/cli/build")

    refute has_element?(view, "[data-part='edit-row']")
    assert has_element?(view, "[data-part='markdown-link']")
  end

  test "guide pages offer a next step and a way back", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/docs/guide/getting-started")

    assert has_element?(view, "nav[aria-label='Adjacent documentation pages']")
    assert has_element?(view, "a[data-part='previous-page'][href='/docs/guide']")
    assert has_element?(view, "a[data-part='next-page'][href='/docs/guide/why']")
  end

  test "answers unknown pages with a 404 and a not-found message", %{conn: conn} do
    html = conn |> get(~p"/docs/nope/missing") |> html_response(404)

    assert html =~ "Page not found"
    assert html =~ ~s(<meta name="robots" content="noindex">)
    refute html =~ ~s(rel="canonical")
  end

  test "a page's head describes that page and names its canonical address", %{conn: conn} do
    why = conn |> get(~p"/docs/guide/why") |> html_response(200)
    start = conn |> get(~p"/docs/guide/getting-started") |> html_response(200)

    description = fn html ->
      [_, content] = Regex.run(~r/<meta name="description" content="([^"]*)"/, html)
      content
    end

    assert String.length(description.(why)) in 40..163
    assert description.(why) != description.(start)
    refute description.(why) == "Struo documentation."

    assert why =~
             ~s(<link rel="canonical" href="#{StruoSiteWeb.Endpoint.url()}/docs/guide/why">)

    refute why =~ ~s(name="robots")

    assert why =~
             ~s(<link rel="alternate" type="text/markdown" href="#{StruoSiteWeb.Endpoint.url()}/docs-markdown/guide/why">)
  end

  test "a page's structured data places it in the docs hierarchy", %{conn: conn} do
    html = conn |> get(~p"/docs/guide/memory/evidence") |> html_response(200)

    [json] =
      Regex.run(~r{<script type="application/ld\+json">(.*?)</script>}s, html,
        capture: :all_but_first
      )

    graph = Jason.decode!(json)["@graph"]
    site = StruoSiteWeb.Endpoint.url()

    assert %{"headline" => "Query Action Evidence"} =
             Enum.find(graph, &(&1["@type"] == "TechArticle"))

    assert Enum.find(graph, &(&1["@type"] == "BreadcrumbList"))["itemListElement"]
           |> Enum.map(&{&1["position"], &1["name"], &1["item"]}) == [
             {1, "Documentation", site <> "/docs"},
             {2, "Memory", site <> "/docs/guide/memory"},
             {3, "Evidence", site <> "/docs/guide/memory/evidence"}
           ]
  end

  test "serves raw markdown", %{conn: conn} do
    conn = get(conn, ~p"/docs-markdown/guide/why")

    assert response_content_type(conn, :md)
    assert response(conn, 200) =~ "#"
  end
end
