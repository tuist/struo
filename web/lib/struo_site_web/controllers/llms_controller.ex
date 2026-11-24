defmodule StruoSiteWeb.LlmsController do
  @moduledoc """
  Serves `/llms.txt`: the documentation index in the https://llmstxt.org format,
  so coding agents and answer engines find each page's Markdown source and what
  it covers. Sections and order follow the docs sidebar.
  """
  use StruoSiteWeb, :controller

  alias StruoSiteWeb.Docs
  alias StruoSiteWeb.Docs.OgImage
  alias StruoSiteWeb.Docs.Sidebar
  alias StruoSiteWeb.Endpoint
  alias StruoSiteWeb.Layouts

  def index(conn, _params) do
    conn
    |> put_resp_content_type("text/plain")
    |> send_resp(200, render_index())
  end

  defp render_index do
    sections =
      Enum.map_join(Sidebar.guide_tree() ++ Sidebar.reference_tree(), "\n", fn group ->
        links =
          group
          |> Sidebar.pages()
          |> Enum.flat_map(&link/1)

        if links == [], do: "", else: "## #{group.label}\n\n#{Enum.join(links, "\n")}\n"
      end)

    """
    # Struo

    > #{Layouts.default_description()}

    Every documentation page is also available as Markdown at the linked URLs.
    Start with Getting Started, then follow the guide for your scripts, native
    project, or toolchain. Use the reference for exact commands, manifest
    fields, and target kinds.

    #{sections}\
    """
  end

  defp link(%{slug: "/docs/" <> path, label: label}) do
    case Docs.get_page(String.split(path, "/")) do
      {:ok, page} ->
        url = Endpoint.url() <> "/docs-markdown/" <> path
        title = page.title || label

        description =
          if page.description, do: ": " <> OgImage.summarize(page.description, 200), else: ""

        ["- [#{title}](#{url})#{description}"]

      :error ->
        []
    end
  end
end
